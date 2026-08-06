//! Clock semantics for the `RouteSurface` compaction domain.
//!
//! The domain's clock tracks the augmentation WORLD, not the
//! augmentation INDEX. That distinction is the whole design: the index
//! is populated, self-healed and repopulated CONSTANTLY, and — unlike
//! the semantic-import store — it does so from INSIDE active fact
//! tracers on the same thread. A clock that advanced for index churn
//! would make the domain refuse its own consumers' cold work, forking
//! every follower for exactly the large entries compaction exists to
//! make reusable.
//!
//! So the no-advance half is not a nicety here; it is the reason the
//! domain is armable at all. It is asserted first.

#![cfg(not(target_arch = "wasm32"))]

use std::sync::Arc;

use smallvec::SmallVec;

use crate::file_artifact_store::{
    AugmentationPopulation, AugmentationTargetKey, AugmentationTargetKind, AugmenterSet,
};
use crate::types::{FileLanguage, HostConfig, UpsertRequest};
use crate::VerterHost;

fn host_with_an_augmenter() -> Arc<VerterHost> {
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
    // The augmentation index scans INDEXED artifacts, so the augmenter
    // must be indexed before its contribution is discoverable.
    for path in ["/types.ts", "/aug.ts"] {
        host.ensure_indexed_ready(path)
            .unwrap_or_else(|| panic!("fixture invariant: {path} must index"));
    }
    host
}

fn target_key(host: &VerterHost, target: &str) -> AugmentationTargetKey {
    let env = host.host_view_env_hashes();
    AugmentationTargetKey {
        project_identity: host.host_view_project_identity(),
        resolve_env_hash: env.resolve_env_hash,
        lib_env_hash: env.lib_env_hash,
        population: AugmentationPopulation::Base,
        target: AugmentationTargetKind::ResolvedRelativeCanonical(Arc::from(target)),
    }
}

/// Populate the index row for `target`, exactly as the production cold
/// stitch does.
fn materialize_row(host: &VerterHost, target: &str) -> Arc<AugmenterSet> {
    let key = target_key(host, target);
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
}

fn clock(host: &VerterHost) -> u64 {
    host.project_type_store()
        .indexed()
        .stable_route_surface_generation()
        .expect("a quiescent store reports a stable route-surface generation")
}

/// **First-time materialisation of an index row does NOT advance the
/// clock.**
///
/// The row is new; the augmentation world is not. This is the case that
/// dominates in practice — a measured 96.9% of cold index installs
/// happen inside an active fact tracer — so counting it as a semantic
/// advance would make a scope that merely POPULATED the index refuse its
/// own admission.
///
/// Mutation recipe, EXECUTED: in
/// `FileArtifactStore::publish_augmenter_set`, change `is_some_and` to
/// `is_none_or` — the `artifact_generation` rule, which treats
/// absent → present as a change. ONLY this test fails; the
/// same-fingerprint sibling below is untouched, because that plant
/// moves the `None` arm alone.
#[test]
fn first_time_materialisation_does_not_advance_the_clock() {
    let host = host_with_an_augmenter();
    let before = clock(&host);

    let set = materialize_row(&host, "/types.ts");
    assert!(
        !set.entries.is_empty(),
        "fixture invariant: the augmenter must actually target this module, or the row is empty \
         and this test says nothing about a real materialisation"
    );

    assert_eq!(
        clock(&host),
        before,
        "materialising an index row from an unchanged artifact corpus is a CACHE population, not \
         a change to the augmentation world. Advancing here would make every scope that warms \
         the index refuse its own admission — and the index warms inside active fact tracers"
    );
}

/// A same-fingerprint republish — the stale-key self-heal shape — does
/// NOT advance the clock.
///
/// Every recorded shape fact stays valid by construction (the
/// fingerprint is what they record), and an older captured root still
/// resolves the retired version through the version chain, so
/// birth-epoch movement alone is not a validity flip.
///
/// Mutation recipe, EXECUTED: make `publish_augmenter_set`'s `changed`
/// unconditionally `true`. This test and the first-time-materialisation
/// sibling both fail, while the fingerprint-CHANGE control stays green —
/// the pair that separates "advances for a real change" from "advances
/// for every publication".
#[test]
fn a_same_fingerprint_republish_does_not_advance_the_clock() {
    let host = host_with_an_augmenter();
    let key = target_key(&host, "/types.ts");
    let set = materialize_row(&host, "/types.ts");
    let before = clock(&host);

    // Republish byte-identically, exactly as the self-heal does.
    let republished = Arc::new(AugmenterSet {
        entries: set.entries.clone(),
        fingerprint: set.fingerprint,
    });
    let prev = host
        .project_type_store()
        .indexed()
        .populate_augmenter_set(key, republished);
    assert!(
        prev.is_some(),
        "fixture invariant: a row must already have been published, or this is a first-time \
         materialisation and not a republish"
    );

    assert_eq!(
        clock(&host),
        before,
        "a republish under the IDENTICAL fingerprint changes nothing a recorded shape fact \
         depends on"
    );
}

/// **Replacing a published set with a DIFFERENT fingerprint DOES advance
/// the clock.**
///
/// The control for both no-advance tests above: without it they are
/// satisfied by a clock that never moves at all.
///
/// Mutation recipe: return `(prev, false)` unconditionally from
/// `publish_augmenter_set`'s `mutate` closure. This test fails while the
/// two no-advance tests stay green.
#[test]
fn replacing_a_published_set_with_a_different_fingerprint_advances_the_clock() {
    let host = host_with_an_augmenter();
    let key = target_key(&host, "/types.ts");
    let set = materialize_row(&host, "/types.ts");
    let before = clock(&host);

    let mut different = [0_u8; 16];
    different.copy_from_slice(&set.fingerprint);
    different[0] = different[0].wrapping_add(1);
    assert_ne!(
        different, set.fingerprint,
        "fixture invariant: the replacement fingerprint must actually differ"
    );

    host.project_type_store().indexed().populate_augmenter_set(
        key,
        Arc::new(AugmenterSet {
            entries: SmallVec::new(),
            fingerprint: different,
        }),
    );

    assert_ne!(
        clock(&host),
        before,
        "the published augmenter set for this target changed, so every witness that compacted \
         the domain beforehand describes a set that is no longer installed"
    );
}

/// Retiring index contributors advances the clock.
///
/// Retirement removes augmenters from the world, which is a semantic
/// change and not index churn — the asymmetry with publication is
/// deliberate.
///
/// Mutation recipe: return `(removed, false)` from the `mutate` closure
/// in `invalidate_augmentation_index_at_epoch`. This test fails.
#[test]
fn retiring_index_contributors_advances_the_clock() {
    let host = host_with_an_augmenter();
    let set = materialize_row(&host, "/types.ts");
    assert!(
        !set.entries.is_empty(),
        "fixture invariant: there must be a contributor to retire"
    );
    let before = clock(&host);

    // Edit the augmenter: the upsert retires its prior artifact, which
    // retires every index entry that artifact contributed to.
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some("/aug.ts".to_string()),
            input_id: "/aug.ts".to_string(),
            source: Arc::from(
                "declare module './types' { interface Base { c: boolean } }\nexport {}\n",
            ),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("edit must succeed");
    // Publishing the edited augmenter's artifact RETIRES the superseded
    // one, and that retirement is what removes the index entries it
    // contributed to.
    host.ensure_indexed_ready("/aug.ts")
        .expect("the edited augmenter must re-index");

    assert_ne!(
        clock(&host),
        before,
        "an augmenter's contribution was retired, so a witness that compacted the domain \
         beforehand vouches for a contributor set the index no longer holds"
    );
}

/// Clearing the index is CACHE-only and does not advance the clock.
///
/// The augmentation world is unchanged — the index rebuilds identically
/// from the same artifact corpus — so a compacted witness stays valid.
/// Any world change that PRECEDED the clear advanced the clock in its
/// own right.
///
/// Mutation recipe: wrap `clear_augmentation_index`'s
/// `retire_augmenter_keys` call in `route_surface_generation.mutate(||
/// (.., true))`. This test fails.
#[test]
fn clearing_the_index_is_cache_only_and_does_not_advance_the_clock() {
    let host = host_with_an_augmenter();
    let _ = materialize_row(&host, "/types.ts");
    let before = clock(&host);
    assert!(
        host.project_type_store().indexed().augmentation_index_len() > 0,
        "fixture invariant: the index must hold something for the clear to remove"
    );

    host.project_type_store()
        .indexed()
        .clear_augmentation_index();

    assert_eq!(
        clock(&host),
        before,
        "a clear drops cached rows the next cold scan rebuilds identically from an unchanged \
         artifact corpus; treating it as a world change would invalidate every compacted \
         route-surface witness for a pure cache operation"
    );
}
