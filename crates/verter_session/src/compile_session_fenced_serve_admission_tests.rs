//! Session compile-slot fenced-serve admission: ReturnOnly never
//! publishes.
//!
//! The `Session`-mode compile cold path installs the fact tracer around
//! `observe_compile_tier_dependencies` + `compile_entry`. A compile
//! whose traced scope consumed a FENCED (ReturnOnly,
//! `store_published == false`) `IndexedReady` serve derived its output
//! from a served-without-publication artifact while its fact stamps are
//! read from the LIVE post-mutation state — an entry the read-side fact
//! rail cannot reject. The admission consults the tracer's by-value
//! `fenced_serve_observed` flag and DECLINES the shared-cache publish:
//! no session slot, no scheduler artifact snapshot. The caller is still
//! served the freshly compiled output (its request pre-dates the
//! mutation).

use std::sync::Arc;

use crate::hash::compile_profile_hash;
use crate::types::{
    CompileCacheMode, CompileProfile, HostConfig, UpsertRequest, VirtualNodeKind, VirtualQuery,
};
use crate::VerterHost;

const OWNER: &str = "/proj/Comp.vue";
const DEP: &str = "/proj/dep.ts";
const OTHER: &str = "/proj/other.ts";

/// An SFC whose `defineProps<P>()` payload is an IMPORTED type: the
/// compile's external macro-type collection walks the frontier to the
/// dep, so the compile flight's traced scope consumes `IndexedReady`
/// serves for `DEP` — the surface through which a fenced serve reaches
/// the compile tracer.
const OWNER_SOURCE: &str = "<script setup lang=\"ts\">\nimport type { P } from './dep';\ndefineProps<P>()\n</script>\n<template><div /></template>";

fn make_host() -> Arc<VerterHost> {
    let workspace = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    Arc::new(VerterHost::new(HostConfig::default(), workspace))
}

fn upsert(host: &VerterHost, canonical_id: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(canonical_id.to_string()),
            input_id: canonical_id.to_string(),
            source: Arc::from(source),
            file_language: verter_language::LanguageRegistry::global()
                .classify_static(canonical_id)
                .static_resolution(),
            aliases: Vec::new(),
        })
        .expect("upsert must succeed");
}

fn upsert_fixture(host: &VerterHost) {
    upsert(host, DEP, "export type P = { msg: string };\n");
    upsert(host, OTHER, "export type Other = { o: 1 };\n");
    upsert(host, OWNER, OWNER_SOURCE);
}

fn compile(host: &VerterHost) -> crate::types::VirtualFileResponse {
    host.get_virtual_file(VirtualQuery {
        raw_id: None,
        canonical_id: Some(OWNER.to_string()),
        node_kind: Some(VirtualNodeKind::Main),
        compile_profile: CompileProfile::default(),
    })
    .expect("compile must serve")
}

/// Whether the Session compile slot for `(OWNER, default profile)` holds
/// a published entry — the primary observable the fenced-serve admission
/// gates.
fn session_slot_present(host: &VerterHost) -> bool {
    let profile_hash = compile_profile_hash(&CompileProfile::default());
    host.compile_cache()
        .get(OWNER)
        .map(|cc| {
            crate::cache_runtime::CompileOutputNodeFactValidatedSession::new()
                .peek_signature(&cc, profile_hash)
                .is_some()
        })
        .unwrap_or(false)
}

/// Arm the materialize seam so EVERY `IndexedReady` materialise flight
/// in the next request window is FENCED: each seam fire lands a fresh
/// (value-distinct, so the changed-gate cannot skip the cascade)
/// exact-resolution push on the unrelated canonical `OTHER`, bumping
/// `project_generation` between the flight's stamp capture and its
/// pre-publish fence. Every flight therefore serves its caller
/// ReturnOnly (`store_published == false`), and the serve consumed
/// INSIDE the compile's traced scope raises the tracer's by-value
/// `fenced_serve_observed` chokepoint flag.
fn arm_fence_every_materialize(host: &Arc<VerterHost>) {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    let host_for_hook = Arc::clone(host);
    let fire_count = AtomicUsize::new(0);
    // Re-entrancy guard: the route push below can itself reach a
    // materialise seam (route-mirror refresh); a nested fire must not
    // recurse into another push.
    let in_hook = AtomicBool::new(false);
    *host.materialize_seam_hook.lock() = Some(Arc::new(move || {
        if in_hook.swap(true, Ordering::SeqCst) {
            return;
        }
        let n = fire_count.fetch_add(1, Ordering::SeqCst);
        host_for_hook.set_exact_resolutions(
            OTHER,
            vec![verter_workspace::ExactResolution {
                specifier: format!("./fence_probe_{n}"),
                phase: verter_workspace::ResolvePhase::CodegenBlocker,
                kind: verter_workspace::ResolveRequestKind::TypeImport,
                resolved_canonical_id: Some(DEP.to_string()),
                possible_canonical_ids: vec![DEP.to_string()],
            }],
        );
        in_hook.store(false, Ordering::SeqCst);
    }));
}

/// ReturnOnly never publishes — Session compile-slot arm. A Session
/// compile whose traced scope consumed a FENCED (ReturnOnly)
/// `IndexedReady` serve must DECLINE the mode-routed publish: no
/// session slot, no scheduler artifact snapshot. The caller is still
/// served the freshly compiled output.
///
/// Discrimination: pre-consult, the admission routed ONLY
/// `fact_read_set.finalise()` through `SignatureAdmission` — the
/// fenced compile's facts are read from the LIVE post-mutation state,
/// so `finalise()` returns `Ok` and the slot LANDED (slot present).
/// The fenced consult declines, leaving the slot absent.
#[test]
fn fenced_serve_inside_the_compile_flight_declines_the_session_publish() {
    let host = make_host();
    upsert_fixture(&host);

    arm_fence_every_materialize(&host);
    let fenced = compile(&host);
    *host.materialize_seam_hook.lock() = None;

    // The raced request must really have run as Session (the mode whose
    // publish this test pins) and must still be served its freshly
    // compiled output (ReturnOnly).
    assert_eq!(
        fenced.actual_mode,
        CompileCacheMode::Session,
        "fixture must classify to Session — the fenced-serve pin is otherwise vacuous",
    );
    assert!(!fenced.cache_hit, "first compile must be cold");
    assert!(
        !fenced.code.is_empty(),
        "the declined publish must still serve the freshly compiled output",
    );

    // THE PIN: the compile's traced scope consumed a fenced serve, so
    // the Session publish must DECLINE — no session slot.
    assert!(
        !session_slot_present(&host),
        "a Session compile whose traced scope consumed a FENCED (ReturnOnly) \
         IndexedReady serve must DECLINE the session-slot publish — its fact \
         stamps validate against the live view while its payload was computed \
         from the superseded artifact, an entry the read-side fact rail cannot \
         reject",
    );
    // The companion warm-hit substrate must stay symmetric: no scheduler
    // artifact snapshot either.
    let profile_hash = compile_profile_hash(&CompileProfile::default());
    assert!(
        host.scheduler
            .try_get_artifact(OWNER, profile_hash)
            .is_none(),
        "the declined Session publish must not commit a scheduler artifact \
         snapshot — the companion warm-hit substrate would serve the fenced \
         payload",
    );
    // Third declined observable: the compile-lane raw-template-analysis
    // persist shares the same `is_cacheable` admission, so the fenced
    // compile must leave the shared `derived_raw_cache` template slot
    // empty too.
    assert!(
        host.derived_raw_cache()
            .get(OWNER)
            .and_then(|cc| {
                cc.raw_template_analysis()
                    .map(|entry| Arc::clone(&entry.template))
            })
            .is_none(),
        "the declined Session publish must not persist raw_template_analysis \
         into the shared derived_raw_cache — the entry carries no content \
         rail and every subsequent template read would serve the fenced \
         compile's value as current",
    );

    // Recovery: with the seam disarmed, the next compile cold-rebuilds
    // against live state, publishes, and the one after serves warm —
    // the fenced refusal was the admission gate acting, not a broken
    // publish path.
    let republished = compile(&host);
    assert_eq!(republished.actual_mode, CompileCacheMode::Session);
    assert!(!republished.cache_hit, "no entry exists yet — cold");
    assert!(
        session_slot_present(&host),
        "a quiescent recompile must publish the session slot",
    );
    assert!(
        host.derived_raw_cache()
            .get(OWNER)
            .and_then(|cc| {
                cc.raw_template_analysis()
                    .map(|entry| Arc::clone(&entry.template))
            })
            .is_some(),
        "the quiescent recompile's cacheable admission persists \
         raw_template_analysis — the fenced decline must not suppress \
         the live lane",
    );
    let warm = compile(&host);
    assert!(
        warm.cache_hit,
        "the quiescent recompile's entry must serve the next request warm",
    );
    assert_eq!(
        warm.code, republished.code,
        "warm hit must be byte-identical"
    );
}

/// Negative control: the seam armed but mutating NOTHING must not trip
/// the fenced consult — no fenced serve is consumed, the publish lands,
/// and the next request warm-hits it. Proves the admission consults the
/// fenced-serve flag rather than declining whenever the seam fires.
#[test]
fn unfenced_compile_still_publishes_the_session_slot() {
    let host = make_host();
    upsert_fixture(&host);

    // A no-op hook: every materialise flight passes its fence and
    // publishes normally.
    *host.materialize_seam_hook.lock() = Some(Arc::new(|| {}));
    let cold = compile(&host);
    *host.materialize_seam_hook.lock() = None;

    assert_eq!(cold.actual_mode, CompileCacheMode::Session);
    assert!(!cold.cache_hit, "first compile must be cold");
    assert!(
        session_slot_present(&host),
        "an un-fenced Session compile must publish the session slot",
    );
    let warm = compile(&host);
    assert!(warm.cache_hit, "the published slot must serve warm");
    assert_eq!(warm.code, cold.code, "warm hit must be byte-identical");
}
