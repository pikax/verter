//! Cold per-file artifact-build dedup contract.
//!
//! One cold resolve of one canonical performs exactly ONE eval-program
//! parse, ONE `ShallowFileState` build, and ONE `IndexedReady`
//! materialisation — the `ensure_indexed_ready` materialise closure is
//! the single per-file cold build, every other path reads its output or
//! joins its singleflight. The cold build is INDEX-ONLY: it builds ZERO
//! whole-file `EvalEnv`s (the env is a lazy demand product of the
//! artifact's declaration-body memo — `eval_env_builds` counts those
//! demands). Warm resolves build nothing; route-resolution mutations
//! refresh route edges WITHOUT re-parsing (the edge-refresh path reuses
//! the content-addressed payload).
//!
//! Counters are host-owned `MetaProvenance` atomics (deterministic, no
//! wall-clock); see `types::MetaProvenance` for the counter contract.

use std::sync::Arc;

use crate::semantic_query::ProjectionMode;
use crate::types::{HostConfig, MetaProvenanceSnapshot, UpsertRequest};
use crate::VerterHost;

fn make_host(files: &[(&str, &str)]) -> Arc<VerterHost> {
    let workspace = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    for (path, source) in files {
        workspace.inject_file((*path).to_string(), Arc::from(*source));
    }
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

fn snap(host: &VerterHost) -> MetaProvenanceSnapshot {
    host.provenance().snapshot()
}

/// Bounded cooperative wait for the fence/seam choreography tests: a
/// regression must FAIL with a message, not hang the suite. The deadline
/// is a failure diagnostic only — no behavior is asserted on elapsed
/// time.
#[track_caller]
fn spin_until(label: &str, cond: impl Fn() -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while !cond() {
        assert!(
            std::time::Instant::now() < deadline,
            "bounded wait timed out: {label}",
        );
        std::thread::yield_now();
    }
}

/// Miniature `slow.ts` shape: one requested symbol plus filler
/// intersection decls the resolve must NOT eagerly materialise.
const SCRATCH: &str = "export type Unrelated = { a: 1 };\n\
     type Var0 = { a: 1 };\n\
     type Var1 = Var0;\n\
     type Var2 = Var0 & Var1;\n\
     type Var3 = Var0 & Var1 & Var2;\n";

const SCRATCH_ID: &str = "/workspace/src/scratch.ts";

fn assert_single_build(provenance: &MetaProvenanceSnapshot, label: &str) {
    assert_eq!(
        provenance.eval_program_parses, 1,
        "{label}: exactly one eval-program parse per cold canonical build \
         (got {})",
        provenance.eval_program_parses,
    );
    assert_eq!(
        provenance.eval_env_builds, 0,
        "{label}: a cold canonical build is INDEX-ONLY — the whole-file \
         EvalEnv is a lazy demand product and a per-symbol resolve must \
         never demand it (got {})",
        provenance.eval_env_builds,
    );
    assert_eq!(
        provenance.shallow_state_builds, 1,
        "{label}: exactly one ShallowFileState build per cold canonical build \
         (got {})",
        provenance.shallow_state_builds,
    );
    assert_eq!(
        provenance.indexed_ready_materializes, 1,
        "{label}: exactly one IndexedReady materialisation per cold canonical \
         build (got {})",
        provenance.indexed_ready_materializes,
    );
    assert_eq!(
        provenance.indexed_ready_edge_refreshes, 0,
        "{label}: a cold build must not detour through the edge-refresh lane \
         (got {})",
        provenance.indexed_ready_edge_refreshes,
    );
    // The dedup fixtures are `.ts` files: NO lane may run an SFC
    // structure parse for them (a non-zero count means a host lane
    // misclassified the canonical or re-parsed through the SFC path).
    assert_eq!(
        provenance.sfc_parses, 0,
        "{label}: a non-SFC cold build must perform zero SFC structure \
         parses (got {})",
        provenance.sfc_parses,
    );
    // The cold flight lowers the non-SFC snapshot from the SINGLE
    // eval-program parse (or reuses the scheduler snapshot) — a
    // non-zero count means a lane re-introduced a full
    // `parse_non_sfc_snapshot` re-parse inside the cold build, the
    // exact dead-parse class the dedup contract exists to kill.
    assert_eq!(
        provenance.non_sfc_snapshot_parses, 0,
        "{label}: a cold build must perform zero non-SFC snapshot \
         re-parses (got {})",
        provenance.non_sfc_snapshot_parses,
    );
}

#[test]
fn cold_resolve_builds_each_artifact_once() {
    let host = make_host(&[]);
    upsert(&host, SCRATCH_ID, SCRATCH);
    host.provenance().reset();

    let node =
        host.resolve_named_symbol(SCRATCH_ID, "Unrelated", &[], Some(ProjectionMode::Expanded));
    assert!(node.is_some(), "Unrelated must resolve");

    assert_single_build(&snap(&host), "cold_resolve_builds_each_artifact_once");
}

#[test]
fn warm_resolve_builds_nothing() {
    let host = make_host(&[]);
    upsert(&host, SCRATCH_ID, SCRATCH);

    let cold =
        host.resolve_named_symbol(SCRATCH_ID, "Unrelated", &[], Some(ProjectionMode::Expanded));
    let cold_node = cold.expect("cold resolve must succeed");
    let cold_expr = host
        .project_node_to_type_expr(cold_node)
        .expect("cold node must project");

    host.provenance().reset();
    let warm =
        host.resolve_named_symbol(SCRATCH_ID, "Unrelated", &[], Some(ProjectionMode::Expanded));
    let warm_node = warm.expect("warm resolve must succeed");
    let warm_expr = host
        .project_node_to_type_expr(warm_node)
        .expect("warm node must project");

    let provenance = snap(&host);
    assert_eq!(
        provenance.eval_program_parses, 0,
        "warm resolve must not parse"
    );
    assert_eq!(
        provenance.eval_env_builds, 0,
        "warm resolve must not rebuild the eval env"
    );
    assert_eq!(
        provenance.shallow_state_builds, 0,
        "warm resolve must not rebuild the shallow state"
    );
    assert_eq!(
        provenance.indexed_ready_materializes, 0,
        "warm resolve must not re-materialise IndexedReady"
    );
    assert_eq!(
        format!("{cold_expr:?}"),
        format!("{warm_expr:?}"),
        "warm resolve must return the identical projection"
    );
}

/// `clear_compile_cache` is the LIGHTER lifecycle operation: per-profile
/// compile results flush, but parsed source / analysis snapshots — and
/// the content-addressed `IndexedReady` — are retained. The retained
/// artifact's stamps are untouched (no content or route mutation
/// happened), so it must stay provably FRESH: the next resolve reuses it
/// with zero rebuilds and returns the identical projection. Discriminates
/// both directions — an over-eager artifact clear shows up as non-zero
/// build counters, a freshness-gate break shows up the same way (stale
/// rejection forces a rebuild).
#[test]
fn clear_compile_cache_retains_fresh_indexed_ready() {
    let host = make_host(&[]);
    upsert(&host, SCRATCH_ID, SCRATCH);

    let cold =
        host.resolve_named_symbol(SCRATCH_ID, "Unrelated", &[], Some(ProjectionMode::Expanded));
    let cold_node = cold.expect("cold resolve must succeed");
    let cold_expr = host
        .project_node_to_type_expr(cold_node)
        .expect("cold node must project");
    assert!(
        host.project_type_store()
            .indexed()
            .get_any(SCRATCH_ID)
            .is_some(),
        "precondition: the cold resolve must retain an IndexedReady",
    );

    host.clear_compile_cache();
    assert!(
        host.project_type_store()
            .indexed()
            .get_any(SCRATCH_ID)
            .is_some(),
        "clear_compile_cache must RETAIN the IndexedReady (it flushes \
         compile outputs, not analysis artifacts)",
    );

    host.provenance().reset();
    let warm =
        host.resolve_named_symbol(SCRATCH_ID, "Unrelated", &[], Some(ProjectionMode::Expanded));
    let warm_node = warm.expect("post-clear resolve must succeed");
    let warm_expr = host
        .project_node_to_type_expr(warm_node)
        .expect("post-clear node must project");

    let provenance = snap(&host);
    assert_eq!(
        provenance.eval_program_parses, 0,
        "the retained artifact must serve without a re-parse"
    );
    assert_eq!(
        provenance.eval_env_builds, 0,
        "the retained artifact must serve without an EvalEnv rebuild"
    );
    assert_eq!(
        provenance.shallow_state_builds, 0,
        "the retained artifact must serve without a shallow rebuild"
    );
    assert_eq!(
        provenance.indexed_ready_materializes, 0,
        "the retained artifact must serve without re-materialising"
    );
    assert_eq!(
        provenance.indexed_ready_edge_refreshes, 0,
        "no route/project mutation happened, so no edge refresh either"
    );
    assert_eq!(
        format!("{cold_expr:?}"),
        format!("{warm_expr:?}"),
        "the post-clear resolve must return the identical projection"
    );
}

#[test]
fn concurrent_cold_resolves_collapse() {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    let host = make_host(&[]);
    upsert(&host, SCRATCH_ID, SCRATCH);
    host.provenance().reset();

    // Deterministic leader-park (the overlay collapse test's pattern,
    // applied to the BASE lane): the FIRST flight parks at its
    // materialise seam so the followers' claims are provably concurrent
    // with the leader's build. Without the park, an OS that serializes
    // the four threads lets every follower take the warm fast path and
    // a base-lane singleflight collapse regression passes
    // intermittently.
    let leader_parked = Arc::new(AtomicBool::new(false));
    let release = Arc::new(AtomicBool::new(false));
    let seam_calls = Arc::new(AtomicUsize::new(0));
    {
        let leader_parked = Arc::clone(&leader_parked);
        let release = Arc::clone(&release);
        let seam_calls = Arc::clone(&seam_calls);
        *host.materialize_seam_hook.lock() = Some(Arc::new(move || {
            if seam_calls.fetch_add(1, Ordering::SeqCst) == 0 {
                leader_parked.store(true, Ordering::SeqCst);
                spin_until("seam release", || release.load(Ordering::SeqCst));
            }
        }));
    }

    // The base lane identity is the bare canonical (the overlay lane
    // carries the "overlay\0" prefix — collision-free by construction).
    let token = crate::resolver_core::StoreViewCompatToken {
        epoch: 0,
        session: None,
        validity_fingerprint: 0,
    };

    let results: Vec<_> = std::thread::scope(|scope| {
        let leader = {
            let host = Arc::clone(&host);
            scope.spawn(move || {
                host.ensure_indexed_ready(SCRATCH_ID)
                    .expect("leader cold materialise must succeed")
            })
        };
        spin_until("leader parked", || leader_parked.load(Ordering::SeqCst));
        let followers: Vec<_> = (0..3)
            .map(|_| {
                let host = Arc::clone(&host);
                scope.spawn(move || {
                    host.ensure_indexed_ready(SCRATCH_ID)
                        .expect("follower cold materialise must succeed")
                })
            })
            .collect();
        // Post-fix: the followers JOIN the leader's base lane (strong
        // count grows past the leader-only baseline of 2). Pre-fix
        // there is no lane: each follower runs its own full build,
        // firing its own seam call — terminate the wait on either
        // signal so a regression FAILS (on the single-build assert)
        // instead of hanging.
        spin_until("followers joined the base lane (or ran solo)", || {
            host.resolver
                .runtime
                .indexed_singleflight
                .test_flight_strong_count(&SCRATCH_ID.to_owned(), token)
                >= 5
                || seam_calls.load(Ordering::SeqCst) >= 4
        });
        release.store(true, Ordering::SeqCst);
        let mut results = vec![leader.join().unwrap()];
        results.extend(followers.into_iter().map(|h| h.join().unwrap()));
        results
    });

    let first = &results[0];
    for other in &results[1..] {
        assert!(
            Arc::ptr_eq(first, other),
            "singleflight must hand every concurrent cold caller the same \
             published Arc"
        );
    }
    assert_single_build(&snap(&host), "concurrent_cold_resolves_collapse");
}

/// `.vue` `lang="tsx"` script content must be parsed under the
/// authoritative TSX source type everywhere — including the env the
/// CANONICAL artifact (`IndexedReady.shallow_state`) is built from. A
/// fallback env build hard-coded to `SourceType::ts()` mangles the TSX
/// body and drops the value-symbol inventory from the indexed artifact.
#[test]
fn vue_tsx_sfc_canonical_state_parses_under_authoritative_source_type() {
    let host = make_host(&[]);
    let canonical = "/workspace/src/Comp.vue";
    upsert(
        &host,
        canonical,
        "<script setup lang=\"tsx\">\n\
         const node = <div>hello</div>;\n\
         const plain = 1;\n\
         </script>\n\
         <template><span/></template>\n",
    );
    host.provenance().reset();

    let indexed = host
        .ensure_indexed_ready(canonical)
        .expect("vue canonical must materialise");
    assert!(
        indexed.shallow_state.has_value_symbol("node"),
        "the INDEXED artifact's shallow value inventory must include the \
         TSX-bodied binding (the canonical artifact was built from a \
         worse env than the resolve-route probe); value_symbols = {:?}",
        indexed
            .shallow_state
            .value_symbol_names()
            .collect::<Vec<_>>(),
    );
    let provenance = snap(&host);
    assert_eq!(
        provenance.eval_program_parses, 1,
        "one eval-program parse for the vue canonical build (got {})",
        provenance.eval_program_parses,
    );
    assert_eq!(
        provenance.sfc_parses, 0,
        "the cold build must REUSE the scheduler's SFC structure parse for \
         an upserted .vue — a second `parse_sfc` is exactly the duplicate \
         work the unified build removes (got {})",
        provenance.sfc_parses,
    );
}

/// `<script setup generic="T">` type parameters must reach the env the
/// CANONICAL artifact carries — the single `IndexedReady` build runs
/// `apply_sfc_script_setup_type_params` on the eval env it publishes, so
/// the script-setup generic binding `T` lands on the one artifact every
/// consumer reads.
#[test]
fn vue_generic_sfc_canonical_env_carries_script_setup_type_params() {
    let host = make_host(&[]);
    let canonical = "/workspace/src/Generic.vue";
    upsert(
        &host,
        canonical,
        "<script setup lang=\"ts\" generic=\"T extends string\">\n\
         const props = defineProps<{ value: T }>();\n\
         </script>\n\
         <template><span/></template>\n",
    );
    host.provenance().reset();

    let indexed = host
        .ensure_indexed_ready(canonical)
        .expect("generic vue canonical must materialise");
    assert!(
        indexed
            .shallow_state
            .decl_bodies()
            .whole_env()
            .type_bindings
            .contains_key("T"),
        "the INDEXED artifact's eval env must carry the script-setup \
         generic binding `T`; bindings = {:?}",
        indexed
            .shallow_state
            .decl_bodies()
            .whole_env()
            .type_bindings
            .keys()
            .collect::<Vec<_>>(),
    );
    let provenance = snap(&host);
    assert_eq!(
        provenance.eval_env_builds, 1,
        "one env build for the generic vue canonical (got {})",
        provenance.eval_env_builds,
    );
    assert_eq!(
        provenance.sfc_parses, 0,
        "the cold build must REUSE the scheduler's SFC structure parse for \
         an upserted generic .vue — no second `parse_sfc` (got {})",
        provenance.sfc_parses,
    );
}

#[test]
fn route_then_deepen_is_one_build_per_file() {
    let host = make_host(&[]);
    let barrel = "/workspace/src/barrel.ts";
    let leaf = "/workspace/src/leaf.ts";
    upsert(&host, leaf, "export type Props = { label: string };\n");
    upsert(&host, barrel, "export type { Props } from './leaf';\n");
    host.provenance().reset();

    // Resolve through the barrel, terminal deepening in the leaf.
    let node = host.resolve_named_symbol(barrel, "Props", &[], Some(ProjectionMode::Expanded));
    assert!(node.is_some(), "Props must resolve through the barrel");

    let provenance = snap(&host);
    assert_eq!(
        provenance.eval_program_parses, 2,
        "exactly one eval-program parse per canonical (barrel + leaf), got {}",
        provenance.eval_program_parses,
    );
    assert_eq!(
        provenance.eval_env_builds, 0,
        "route-then-deepen is per-symbol demand — neither canonical's \
         whole-file EvalEnv may build (got {})",
        provenance.eval_env_builds,
    );
    assert_eq!(
        provenance.shallow_state_builds, 2,
        "exactly one ShallowFileState build per canonical (barrel + leaf), got {}",
        provenance.shallow_state_builds,
    );
    assert_eq!(
        provenance.indexed_ready_materializes, 2,
        "exactly one IndexedReady materialisation per canonical (barrel + \
         leaf), got {}",
        provenance.indexed_ready_materializes,
    );
}

#[test]
fn edit_invalidates_exactly_once() {
    let host = make_host(&[]);
    upsert(&host, SCRATCH_ID, SCRATCH);
    let _ = host
        .resolve_named_symbol(SCRATCH_ID, "Unrelated", &[], Some(ProjectionMode::Expanded))
        .expect("cold resolve must succeed");

    upsert(
        &host,
        SCRATCH_ID,
        "export type Unrelated = { a: 2 };\ntype Var0 = { a: 2 };\n",
    );
    host.provenance().reset();
    let node =
        host.resolve_named_symbol(SCRATCH_ID, "Unrelated", &[], Some(ProjectionMode::Expanded));
    assert!(node.is_some(), "post-edit resolve must succeed");

    assert_single_build(&snap(&host), "edit_invalidates_exactly_once");
}

/// A route-resolution mutation (`set_exact_resolutions`) refreshes the
/// canonical's baked route surface WITHOUT a re-parse: the
/// content-addressed payload (source / parse / analysis / env / shallow
/// symbols) is reused and only the route surface (import_routes + route
/// edges + hashes) is rebuilt.
#[test]
fn route_mutation_refreshes_edges_without_reparse() {
    let host = make_host(&[]);
    let owner = "/workspace/src/owner.ts";
    let dep = "/workspace/src/dep.ts";
    let dep2 = "/workspace/src/dep2.ts";
    upsert(&host, dep, "export type P = { a: 1 };\n");
    upsert(&host, dep2, "export type P = { b: 2 };\n");
    upsert(
        &host,
        owner,
        "import type { P } from './dep';\nexport type Owner = P;\n",
    );

    let cold = host
        .ensure_indexed_ready(owner)
        .expect("owner must materialise");
    assert_eq!(
        cold.import_routes
            .get("./dep")
            .and_then(|r| r.resolved_canonical_id.as_deref()),
        Some(dep),
        "pre-mutation route must point at dep.ts"
    );

    host.set_exact_resolutions(
        owner,
        vec![verter_workspace::ExactResolution {
            specifier: "./dep".to_string(),
            phase: verter_workspace::ResolvePhase::CodegenBlocker,
            kind: verter_workspace::ResolveRequestKind::TypeImport,
            resolved_canonical_id: Some(dep2.to_string()),
            possible_canonical_ids: vec![dep2.to_string()],
        }],
    );
    host.provenance().reset();

    let refreshed = host
        .ensure_indexed_ready(owner)
        .expect("owner must re-materialise its route surface");
    assert_eq!(
        refreshed
            .import_routes
            .get("./dep")
            .and_then(|r| r.resolved_canonical_id.as_deref()),
        Some(dep2),
        "post-mutation route must point at dep2.ts"
    );
    let provenance = snap(&host);
    assert_eq!(
        provenance.eval_program_parses, 0,
        "a route mutation must NOT re-parse the owner (content unchanged); \
         the edge-refresh path reuses the content-addressed payload"
    );
    assert_eq!(
        provenance.eval_env_builds, 0,
        "a route mutation must NOT rebuild the eval env — the edge refresh \
         reuses the artifact's canonical env"
    );
    assert_eq!(
        provenance.indexed_ready_edge_refreshes, 1,
        "the route surface must be rebuilt through exactly one edge refresh \
         (got {})",
        provenance.indexed_ready_edge_refreshes,
    );

    // SECOND identical read: the refreshed artifact must have physically
    // LANDED in the store. A republish swallowed by the store's no-op
    // equivalence gate leaves the stored stamp stale, so every subsequent
    // read re-runs the full edge refresh — once-ness demands delta 0 here.
    host.provenance().reset();
    let second = host
        .ensure_indexed_ready(owner)
        .expect("second read must serve the refreshed artifact");
    assert!(
        Arc::ptr_eq(&second, &refreshed),
        "second read must serve the STORED refreshed artifact (fast hit), \
         not a rebuilt one"
    );
    let second_read = snap(&host);
    assert_eq!(
        second_read.indexed_ready_edge_refreshes, 0,
        "the refreshed artifact must serve warm on the second read — a \
         non-zero delta means the republish never landed (got {})",
        second_read.indexed_ready_edge_refreshes,
    );
    assert_eq!(
        second_read.indexed_ready_materializes, 0,
        "the second read must not re-materialise (got {})",
        second_read.indexed_ready_materializes,
    );
}

/// `configure_projects` is a global route-resolution mutation: the next
/// read of a cross-file-edge surface refreshes its route surface from
/// the retained content-addressed payload — no re-parse.
///
/// The no-reparse pin is valid because `configure_projects` does NOT
/// move the owner's `parse_env_hash` (the R21 parse dimension derives
/// from workspace parser flags, not project config — see
/// `IdeProjectConfig::parse_env_hash`), so the parse-reusing edge
/// refresh is the correct route. The parse-env-MOVING case is pinned by
/// `moved_parse_env_forces_full_rematerialise_not_edge_refresh`: a
/// moved parse dimension must re-parse, never edge-refresh.
#[test]
fn project_mutation_refreshes_edges_without_reparse() {
    let host = make_host(&[]);
    let owner = "/workspace/src/owner.ts";
    let dep = "/workspace/src/dep.ts";
    upsert(&host, dep, "export type P = { a: 1 };\n");
    upsert(
        &host,
        owner,
        "import type { P } from './dep';\nexport type Owner = P;\n",
    );
    let _ = host
        .ensure_indexed_ready(owner)
        .expect("owner must materialise");

    host.configure_projects(vec![
        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.json".to_string()),
        ),
    ]);
    host.provenance().reset();

    let refreshed = host
        .ensure_indexed_ready(owner)
        .expect("owner must refresh its route surface");
    assert_eq!(
        refreshed
            .import_routes
            .get("./dep")
            .and_then(|r| r.resolved_canonical_id.as_deref()),
        Some(dep),
        "route target is unchanged by this particular project mutation"
    );
    let provenance = snap(&host);
    assert_eq!(
        provenance.eval_program_parses, 0,
        "a project mutation must NOT re-parse an unchanged canonical"
    );
    assert_eq!(
        provenance.eval_env_builds, 0,
        "a project mutation must NOT rebuild the eval env — the edge refresh \
         reuses the artifact's canonical env"
    );
    assert_eq!(
        provenance.indexed_ready_edge_refreshes, 1,
        "the cross-file-edge surface must refresh exactly once (got {})",
        provenance.indexed_ready_edge_refreshes,
    );

    // SECOND identical read: this mutation did NOT retarget the route, so
    // the refreshed surface is hash-identical to the stale one — exactly
    // the case a stamp-blind store equivalence gate swallows. The stamp-only
    // republish must still land; a non-zero refresh delta here means every
    // future read of every unaffected cross-file-edge canonical re-runs the
    // full edge refresh forever.
    host.provenance().reset();
    let second = host
        .ensure_indexed_ready(owner)
        .expect("second read must serve the refreshed artifact");
    assert!(
        Arc::ptr_eq(&second, &refreshed),
        "second read must serve the STORED refreshed artifact (fast hit), \
         not a rebuilt one"
    );
    let second_read = snap(&host);
    assert_eq!(
        second_read.indexed_ready_edge_refreshes, 0,
        "a stamp-only republish must land — non-zero delta means the store \
         swallowed it and the canonical refreshes per read forever (got {})",
        second_read.indexed_ready_edge_refreshes,
    );
    assert_eq!(
        second_read.indexed_ready_materializes, 0,
        "the second read must not re-materialise (got {})",
        second_read.indexed_ready_materializes,
    );
}

/// The `is_generic_carrier` probe (`shallow_file_state`) must JOIN the
/// canonical `IndexedReady` build — a COLD probe's build IS the
/// IndexedReady build, never a second parallel artifact build.
#[test]
fn probe_reads_canonical_artifact() {
    let host = make_host(&[]);
    upsert(&host, SCRATCH_ID, SCRATCH);
    host.provenance().reset();

    // COLD probe first — the probe must join the singleflighted
    // IndexedReady materialise rather than build a parallel artifact
    // whose Arc the IndexedReady build never reads.
    let probed = host
        .shallow_file_state(SCRATCH_ID)
        .expect("cold probe must return a shallow state");
    let indexed = host
        .ensure_indexed_ready(SCRATCH_ID)
        .expect("indexed artifact must materialise");
    assert!(
        Arc::ptr_eq(&indexed.shallow_state, &probed),
        "a cold shallow probe must join the IndexedReady build and hand \
         out the IndexedReady-owned ShallowFileState Arc, not a parallel \
         artifact"
    );
    let provenance = snap(&host);
    assert_eq!(
        provenance.eval_program_parses, 1,
        "probe + deepen must share ONE eval-program parse (got {})",
        provenance.eval_program_parses,
    );
}

/// Overlay variant of the single-build contract: a session-overlay
/// artifact builds exactly once through the overlay materialiser (one
/// parse, one env, one shallow state, one materialisation) and NEVER
/// touches the base artifact cache — the base canonical keeps no entry
/// under its own (legacy) key for the overlay content.
#[test]
fn overlay_cold_build_is_single_and_never_touches_base_caches() {
    use crate::session_view::OverlaidView;

    let host = make_host(&[]);
    let canonical = "/workspace/src/overlaid.ts";
    upsert(&host, canonical, "export type Base = { a: 1 };\n");
    let base = host
        .ensure_indexed_ready(canonical)
        .expect("base artifact must materialise");

    let mut overlays: rustc_hash::FxHashMap<String, Arc<str>> = rustc_hash::FxHashMap::default();
    overlays.insert(
        canonical.to_string(),
        Arc::from("export type Overlaid = { b: 2 };\n"),
    );
    let view = OverlaidView::new(Arc::clone(&host), overlays);

    host.provenance().reset();
    let first = host
        .materialize_overlay_indexed_ready_with_view(canonical, &view)
        .expect("overlay artifact must materialise");
    assert!(
        first.shallow_state.symbol("Overlaid").is_some(),
        "the overlay artifact must carry the OVERLAY surface"
    );
    assert_single_build(
        &snap(&host),
        "overlay_cold_build_is_single_and_never_touches_base_caches",
    );

    // Warm overlay read: same Arc, zero builds.
    let second = host
        .materialize_overlay_indexed_ready_with_view(canonical, &view)
        .expect("warm overlay read must succeed");
    assert!(
        Arc::ptr_eq(&first, &second),
        "a warm overlay read must reuse the published overlay Arc"
    );
    let provenance = snap(&host);
    assert_eq!(
        provenance.indexed_ready_materializes, 1,
        "the warm overlay read must not re-materialise"
    );

    // Base isolation: the base canonical's own artifact is untouched and
    // still serves the BASE surface.
    let base_again = host
        .ensure_indexed_ready(canonical)
        .expect("base artifact must still resolve");
    assert!(
        Arc::ptr_eq(&base, &base_again),
        "the overlay build must not evict or replace the base artifact"
    );
    assert!(
        base_again.shallow_state.symbol("Overlaid").is_none(),
        "the overlay surface must never leak into the base artifact"
    );
}

/// A singleflight follower whose claim POST-dates a route mutation must
/// never adopt the leader's fenced (ReturnOnly) artifact as current: the
/// fenced flight is not retained, and the follower re-runs against fresh
/// state. Deterministic schedule: the leader parks on the materialise
/// seam with pre-mutation stamps; the mutation lands; the follower joins
/// the parked lane; the leader resumes, trips the pre-publish fence, and
/// broadcasts a ReturnOnly outcome the follower must refuse.
#[test]
fn follower_arriving_after_mutation_does_not_adopt_fenced_flight_result() {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    let host = make_host(&[]);
    let owner = "/workspace/src/owner.ts";
    let dep = "/workspace/src/dep.ts";
    let dep2 = "/workspace/src/dep2.ts";
    upsert(&host, dep, "export type P = { a: 1 };\n");
    upsert(&host, dep2, "export type P = { b: 2 };\n");
    upsert(
        &host,
        owner,
        "import type { P } from './dep';\nexport type Owner = P;\n",
    );
    host.provenance().reset();

    let leader_parked = Arc::new(AtomicBool::new(false));
    let release = Arc::new(AtomicBool::new(false));
    let seam_calls = Arc::new(AtomicUsize::new(0));
    {
        let leader_parked = Arc::clone(&leader_parked);
        let release = Arc::clone(&release);
        let seam_calls = Arc::clone(&seam_calls);
        *host.materialize_seam_hook.lock() = Some(Arc::new(move || {
            // Park ONLY the first flight (the leader). A re-running
            // follower's fresh flight must proceed unimpeded.
            if seam_calls.fetch_add(1, Ordering::SeqCst) == 0 {
                leader_parked.store(true, Ordering::SeqCst);
                spin_until("seam release", || release.load(Ordering::SeqCst));
            }
        }));
    }

    let (leader_result, follower_result) = std::thread::scope(|scope| {
        let leader = {
            let host = Arc::clone(&host);
            scope.spawn(move || host.ensure_indexed_ready(owner))
        };
        spin_until("leader parked", || leader_parked.load(Ordering::SeqCst));
        // The leader is parked INSIDE its flight holding pre-mutation
        // generation stamps. Land the route mutation now.
        host.set_exact_resolutions(
            owner,
            vec![verter_workspace::ExactResolution {
                specifier: "./dep".to_string(),
                phase: verter_workspace::ResolvePhase::CodegenBlocker,
                kind: verter_workspace::ResolveRequestKind::TypeImport,
                resolved_canonical_id: Some(dep2.to_string()),
                possible_canonical_ids: vec![dep2.to_string()],
            }],
        );
        // A POST-mutation follower joins the parked leader's lane.
        let follower = {
            let host = Arc::clone(&host);
            scope.spawn(move || host.ensure_indexed_ready(owner))
        };
        // Deterministic admission (no wall clock): a bare-`run` leader
        // mid-compute holds the leader-only baseline of 2 strong refs on
        // the lane; the committed follower adds its own clone.
        let token = crate::resolver_core::StoreViewCompatToken {
            epoch: 0,
            session: None,
            validity_fingerprint: 0,
        };
        // Rendezvous note: the strong-count >= 3 wait is a HEURISTIC for
        // "the follower committed to the parked lane", but it is
        // fail-closed — while the leader is parked its lane is retained,
        // so a correctly-functioning singleflight FORCES the follower to
        // join; a lane-retention regression leaves the count at the
        // leader-only baseline of 2 and the spin deadline FAILS this
        // test (it never silently passes with degraded choreography).
        // The fenced-outcome class is additionally covered
        // deterministically by
        // `sustained_churn_fallback_serves_return_only_with_admission_suppressed`.
        spin_until("follower joined the parked lane", || {
            host.resolver
                .runtime
                .indexed_singleflight
                .test_flight_strong_count(&owner.to_string(), token)
                >= 3
        });
        release.store(true, Ordering::SeqCst);
        (
            leader.join().unwrap().expect("leader must serve a result"),
            follower
                .join()
                .unwrap()
                .expect("follower must serve a result"),
        )
    });

    // The leader's own caller predates the mutation: ReturnOnly serving
    // is its contract. The FOLLOWER arrived after the mutation, so its
    // result must be the store-current, fence-passed surface.
    assert!(
        host.indexed_surface_is_current(owner, &follower_result),
        "a follower that arrived after the mutation must never adopt the \
         leader's fenced (superseded) artifact as current",
    );
    assert_eq!(
        follower_result
            .import_routes
            .get("./dep")
            .and_then(|r| r.resolved_canonical_id.as_deref()),
        Some(dep2),
        "the post-mutation follower must observe the post-mutation route \
         table",
    );
    // The fenced leader published NOTHING (ReturnOnly never publishes);
    // the store's current artifact is the follower's re-run product.
    let published = host
        .project_type_store()
        .indexed()
        .get(owner, follower_result.whole_hash)
        .expect("the follower's re-run must have published a current artifact");
    assert!(
        Arc::ptr_eq(&published, &follower_result),
        "the follower must serve the artifact its re-run published",
    );
    let _ = leader_result;
}

/// The R21 parse dimension on the reuse gates (the successor of the
/// retired full-env-dimensions env-cache key pin): an artifact whose
/// `parse_env_hash` stamp no longer matches the
/// owner's live parse env must take the FULL re-materialise (re-parse
/// under the live env) — never the parse-reusing edge refresh, never
/// the route-insensitive no-edge reuse. The live base parse env is
/// host-constant today (constant workspace parser flags), so the moved
/// dimension is driven by forging the stored artifact's stamp; the
/// symmetric control proves an EQUAL stamp still takes the edge
/// refresh.
///
/// COUPLING NOTE: the forged candidate is a field-by-field clone
/// re-inserted through a direct `indexed().insert` — deliberately
/// bypassing the publisher. If `IndexedReady` gains a NEW field with
/// reuse-gate semantics, this clone must forge/copy it too, or the pin
/// silently decouples from the real publisher's shape (the clone is
/// exhaustive-by-construction via struct-literal syntax: a new field is
/// a COMPILE error here, which is the tripwire).
#[test]
fn moved_parse_env_forces_full_rematerialise_not_edge_refresh() {
    let host = make_host(&[]);
    let owner = "/workspace/src/owner.ts";
    let dep = "/workspace/src/dep.ts";
    upsert(&host, dep, "export type P = { a: 1 };\n");
    upsert(
        &host,
        owner,
        "import type { P } from './dep';\nexport type Owner = P;\n",
    );
    let built = host
        .ensure_indexed_ready(owner)
        .expect("owner must materialise");
    let live_env = host.host_view_env_hashes_for(owner).parse_env_hash;
    assert_eq!(
        built.parse_env_hash, live_env,
        "a fresh artifact must carry the live parse-env stamp",
    );

    // Forge a stored candidate whose parse env MOVED (and whose project
    // stamp is stale so the reuse gates actually consult the dimension).
    let forge = |parse_env_hash: crate::types::Hash16| crate::project_type_store::IndexedReady {
        whole_hash: built.whole_hash,
        shallow_state: Arc::clone(&built.shallow_state),
        import_routes: Arc::clone(&built.import_routes),
        import_route_hash: built.import_route_hash,
        route_hash: built.route_hash,
        edge_generation: built.edge_generation,
        project_generation: built.project_generation.wrapping_sub(1),
        parse_env_hash,
        raw_source: Arc::clone(&built.raw_source),
        eval_source: Arc::clone(&built.eval_source),
        framework_parse: built.framework_parse.clone(),
        script_analysis: built.script_analysis.clone(),
        export_signatures: built.export_signatures.clone(),
        snapshot: Arc::clone(&built.snapshot),
        external_type_analysis: Arc::clone(&built.external_type_analysis),
        declares_interface_app_config: built.declares_interface_app_config,
        macro_hot_mirror: crate::macro_hot_mirror::MacroHotMirror::default(),
    };
    let mut moved_env = live_env;
    moved_env[0] = moved_env[0].wrapping_add(1);
    assert_ne!(moved_env, live_env, "anti-vacuity: the forged env moved");
    host.project_type_store()
        .indexed()
        .insert(Arc::from(owner), Arc::new(forge(moved_env)));

    host.provenance().reset();
    let rebuilt = host
        .ensure_indexed_ready(owner)
        .expect("owner must re-materialise under the live parse env");
    let provenance = snap(&host);
    // The rebuild reuses the lowering service's RETAINED snapshot: it was
    // parsed under the LIVE parse env at first build, and the retention
    // key (`SnapshotKey`) carries `parse_env_hash`, so only a parse made
    // under the SAME live env can answer (a snapshot parsed under a moved
    // env can never serve — pinned by the decl_lowering service test
    // `moved_parse_env_key_forces_fresh_parse`).
    assert_eq!(
        provenance.eval_program_parses, 0,
        "the full rematerialise reuses the live-env retained snapshot \
         (got {} parses)",
        provenance.eval_program_parses,
    );
    assert_eq!(
        provenance.indexed_ready_materializes, 1,
        "a moved parse env must force a FULL rematerialise"
    );
    assert_eq!(
        provenance.indexed_ready_edge_refreshes, 0,
        "a moved parse env must NOT take the parse-reusing edge refresh"
    );
    assert_eq!(
        rebuilt.parse_env_hash, live_env,
        "the rebuilt artifact carries the live parse-env stamp"
    );

    // Symmetric control: an EQUAL parse-env stamp with the same stale
    // project stamp still takes the edge refresh (no re-parse).
    host.project_type_store()
        .indexed()
        .insert(Arc::from(owner), Arc::new(forge(live_env)));
    host.provenance().reset();
    let refreshed = host
        .ensure_indexed_ready(owner)
        .expect("owner must refresh its route surface");
    let control = snap(&host);
    assert_eq!(
        control.eval_program_parses, 0,
        "an unmoved parse env must reuse the retained parse (edge refresh)"
    );
    assert_eq!(
        control.indexed_ready_edge_refreshes, 1,
        "an unmoved parse env with a stale project stamp takes the edge \
         refresh"
    );
    assert!(
        host.indexed_surface_is_current(owner, &refreshed),
        "the refreshed surface is current"
    );
}

/// The edge-refresh publish fence covers the parse-env reuse gate: the
/// flight's fence generations are captured BEFORE the gate compares the
/// live parse env against the candidate's stamp, so a parse-env-moving
/// mutation (which always bumps `project_generation`) landing between
/// the gate and the publish DECLINES the refresh publish (ReturnOnly)
/// instead of landing an artifact that pairs a CURRENT
/// `project_generation` stamp with a payload parsed under the
/// superseded env. That pairing is forged-current:
/// `indexed_surface_is_current` short-circuits on a current project
/// stamp as proof of parse-env currency, so the entry would warm-serve
/// the stale-parsed payload indefinitely — unrejectable read-side.
///
/// The live parse env is driven through the test-only
/// `parse_env_override` (the production parse dimension derives solely
/// from constant workspace parser flags today, so no public mutation
/// can move it); the override flip is paired with a route-resolution
/// push so the emulated mutation bumps `project_generation` exactly as
/// every real parse-env-moving mutation does.
#[test]
fn parse_env_mutation_between_reuse_gate_and_refresh_declines_the_edge_refresh_publish() {
    use std::sync::atomic::{AtomicBool, Ordering};

    let host = make_host(&[]);
    let owner = "/workspace/src/owner.ts";
    let dep = "/workspace/src/dep.ts";
    let other = "/workspace/src/other.ts";
    upsert(&host, dep, "export type P = { a: 1 };\n");
    upsert(&host, other, "export type Other = { o: 1 };\n");
    upsert(
        &host,
        owner,
        "import type { P } from './dep';\nexport type Owner = P;\n",
    );
    let built = host
        .ensure_indexed_ready(owner)
        .expect("owner must materialise");
    let baseline_env = host.host_view_env_hashes_for(owner).parse_env_hash;
    assert_eq!(
        built.parse_env_hash, baseline_env,
        "anti-vacuity: a fresh artifact carries the live parse-env stamp",
    );

    // Stale the candidate's project stamp WITHOUT moving content or
    // parse env so the next read takes the edge-refresh arm.
    land_unrelated_route_mutation(&host, other, dep);

    let mut moved_env = baseline_env;
    moved_env[0] = moved_env[0].wrapping_add(1);

    // Arm the gate seam: AFTER the reuse gate compared the live parse
    // env against the candidate's stamp, land the parse-env-moving
    // mutation — the override flip plus a value-distinct route push so
    // `project_generation` bumps, exactly the coupling every real
    // parse-env-moving mutation carries.
    let seam_fired = Arc::new(AtomicBool::new(false));
    {
        let host_for_hook = Arc::clone(&host);
        let seam_fired = Arc::clone(&seam_fired);
        // Re-entrancy guard: the route push below can itself reach a
        // gated read; a nested fire must not recurse into another push.
        let in_hook = AtomicBool::new(false);
        *host.edge_refresh_gate_seam_hook.lock() = Some(Arc::new(move || {
            if in_hook.swap(true, Ordering::SeqCst) {
                return;
            }
            seam_fired.store(true, Ordering::SeqCst);
            *host_for_hook.parse_env_override.lock() = Some(moved_env);
            host_for_hook.set_exact_resolutions(
                other,
                vec![verter_workspace::ExactResolution {
                    specifier: "./fence_probe_parse_env".to_string(),
                    phase: verter_workspace::ResolvePhase::CodegenBlocker,
                    kind: verter_workspace::ResolveRequestKind::TypeImport,
                    resolved_canonical_id: Some(dep.to_string()),
                    possible_canonical_ids: vec![dep.to_string()],
                }],
            );
            in_hook.store(false, Ordering::SeqCst);
        }));
    }

    let _served = host
        .ensure_indexed_ready(owner)
        .expect("the raced read must still serve its caller");
    *host.edge_refresh_gate_seam_hook.lock() = None;
    assert!(
        seam_fired.load(Ordering::SeqCst),
        "anti-vacuity: the raced read must have taken the edge-refresh arm",
    );
    let live_env = host.host_view_env_hashes_for(owner).parse_env_hash;
    assert_eq!(
        live_env, moved_env,
        "anti-vacuity: the live parse env moved"
    );

    // THE PIN: no stored artifact may pair a CURRENT project stamp with
    // a parse-env stamp that differs from the live env — the
    // forged-current combination the reader gate short-circuits on
    // (current project stamp ⇒ parse env unmoved).
    if let Some(stored) = host
        .project_type_store()
        .indexed()
        .get(owner, built.whole_hash)
    {
        let current_generation = host.project_type_store().current_project_generation();
        assert!(
            !(stored.project_generation == current_generation && stored.parse_env_hash != live_env),
            "the raced edge refresh published a forged-current artifact: a \
             CURRENT project stamp over a payload parsed under the \
             superseded env — the reader gate short-circuits on the project \
             stamp as proof of parse-env currency, so this entry warm-serves \
             stale parse state unrejectably",
        );
    }

    // Recovery: the next read must re-materialise under the live env
    // (full re-parse), publish, and the read after must warm-hit it.
    host.provenance().reset();
    let recovered = host
        .ensure_indexed_ready(owner)
        .expect("the post-mutation read must serve");
    assert_eq!(
        recovered.parse_env_hash, live_env,
        "the post-mutation read must serve an artifact parsed under the \
         LIVE env, never the stale-parsed payload",
    );
    let provenance = snap(&host);
    assert_eq!(
        provenance.eval_program_parses, 1,
        "the moved parse env must force the full re-materialise (re-parse \
         under the live env)",
    );
    assert_eq!(
        provenance.indexed_ready_edge_refreshes, 0,
        "the moved parse env must never take the parse-reusing edge refresh",
    );
    host.provenance().reset();
    let warm = host
        .ensure_indexed_ready(owner)
        .expect("the warm read must serve");
    assert!(
        Arc::ptr_eq(&warm, &recovered),
        "the recovered artifact must have published and serve warm",
    );
    assert_eq!(
        snap(&host).indexed_ready_materializes,
        0,
        "the warm read must not re-materialise",
    );
}

/// Negative control: the gate seam armed with a NO-OP hook must not
/// change the edge-refresh outcome — the refresh publishes, no
/// re-parse, and the second read serves the stored refreshed artifact
/// warm. Proves the raced decline above is the fence acting on the
/// mutation, not the seam perturbing the flight.
#[test]
fn unmoved_parse_env_through_the_gate_seam_still_publishes_the_edge_refresh() {
    let host = make_host(&[]);
    let owner = "/workspace/src/owner.ts";
    let dep = "/workspace/src/dep.ts";
    let other = "/workspace/src/other.ts";
    upsert(&host, dep, "export type P = { a: 1 };\n");
    upsert(&host, other, "export type Other = { o: 1 };\n");
    upsert(
        &host,
        owner,
        "import type { P } from './dep';\nexport type Owner = P;\n",
    );
    let _built = host
        .ensure_indexed_ready(owner)
        .expect("owner must materialise");
    land_unrelated_route_mutation(&host, other, dep);

    *host.edge_refresh_gate_seam_hook.lock() = Some(Arc::new(|| {}));
    host.provenance().reset();
    let refreshed = host
        .ensure_indexed_ready(owner)
        .expect("the gated read must serve");
    *host.edge_refresh_gate_seam_hook.lock() = None;

    let provenance = snap(&host);
    assert_eq!(
        provenance.indexed_ready_edge_refreshes, 1,
        "the project-stale read takes exactly one edge refresh",
    );
    assert_eq!(
        provenance.eval_program_parses, 0,
        "an unmoved parse env reuses the retained parse",
    );

    host.provenance().reset();
    let second = host
        .ensure_indexed_ready(owner)
        .expect("the second read must serve");
    assert!(
        Arc::ptr_eq(&second, &refreshed),
        "the refreshed artifact must have published and serve warm",
    );
    assert_eq!(
        snap(&host).indexed_ready_edge_refreshes,
        0,
        "the published refresh serves warm — no second refresh",
    );
}

/// Sustained-churn bounded fallback: a claimant that loses the lane
/// election on EVERY bounded attempt, with every won flight fenced by a
/// fresh mutation, is finally served the last fenced artifact ReturnOnly.
/// That serve must carry its non-admissible status to the enclosing cold
/// compute: the claimant's request POST-dates the supersessions, so a
/// downstream compute would record live facts while having computed from
/// superseded data — a poisoned admission the read-side fact rail cannot
/// catch. Deterministic schedule: three sequential leaders each park at
/// the pre-fence seam, a mutation lands, the follower (parked on the
/// retry seam between attempts) re-joins each fresh lane.
#[test]
fn sustained_churn_fallback_serves_return_only_with_admission_suppressed() {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    let host = make_host(&[]);
    let owner = "/workspace/src/owner.ts";
    let dep = "/workspace/src/dep.ts";
    let dep2 = "/workspace/src/dep2.ts";
    upsert(&host, dep, "export type P = { a: 1 };\n");
    upsert(&host, dep2, "export type P = { b: 2 };\n");
    upsert(
        &host,
        owner,
        "import type { P } from './dep';\nexport type Owner = P;\n",
    );
    host.provenance().reset();

    // Materialise seam: flight k parks at its PRE-FENCE call (global odd
    // call index 2k+1) until its release flag flips.
    let flight_parked: Arc<Vec<AtomicBool>> =
        Arc::new((0..3).map(|_| AtomicBool::new(false)).collect());
    let flight_release: Arc<Vec<AtomicBool>> =
        Arc::new((0..3).map(|_| AtomicBool::new(false)).collect());
    {
        let seam_calls = Arc::new(AtomicUsize::new(0));
        let flight_parked = Arc::clone(&flight_parked);
        let flight_release = Arc::clone(&flight_release);
        *host.materialize_seam_hook.lock() = Some(Arc::new(move || {
            let n = seam_calls.fetch_add(1, Ordering::SeqCst);
            if n % 2 == 1 {
                let k = n / 2;
                if let (Some(parked), Some(release)) = (flight_parked.get(k), flight_release.get(k))
                {
                    parked.store(true, Ordering::SeqCst);
                    spin_until("seam release", || release.load(Ordering::SeqCst));
                }
            }
        }));
    }
    // Retry seam: the follower parks after each of its first two fenced
    // attempts until the next leader holds a fresh lane.
    let retry_parked: Arc<Vec<AtomicBool>> =
        Arc::new((0..2).map(|_| AtomicBool::new(false)).collect());
    let retry_release: Arc<Vec<AtomicBool>> =
        Arc::new((0..2).map(|_| AtomicBool::new(false)).collect());
    {
        let retry_calls = Arc::new(AtomicUsize::new(0));
        let retry_parked = Arc::clone(&retry_parked);
        let retry_release = Arc::clone(&retry_release);
        *host.flight_retry_seam_hook.lock() = Some(Arc::new(move || {
            let n = retry_calls.fetch_add(1, Ordering::SeqCst);
            if let (Some(parked), Some(release)) = (retry_parked.get(n), retry_release.get(n)) {
                parked.store(true, Ordering::SeqCst);
                spin_until("seam release", || release.load(Ordering::SeqCst));
            }
        }));
    }

    let token = crate::resolver_core::StoreViewCompatToken {
        epoch: 0,
        session: None,
        validity_fingerprint: 0,
    };
    let lane_count = |host: &VerterHost| -> usize {
        host.resolver
            .runtime
            .indexed_singleflight
            .test_flight_strong_count(&owner.to_string(), token)
    };
    let mutate = |host: &VerterHost, target: &str| {
        host.set_exact_resolutions(
            owner,
            vec![verter_workspace::ExactResolution {
                specifier: "./dep".to_string(),
                phase: verter_workspace::ResolvePhase::CodegenBlocker,
                kind: verter_workspace::ResolveRequestKind::TypeImport,
                resolved_canonical_id: Some(target.to_string()),
                possible_canonical_ids: vec![target.to_string()],
            }],
        );
    };

    let (follower_result, follower_partial) = std::thread::scope(|scope| {
        // Leader 1 claims the lane and parks pre-fence.
        let l1 = {
            let host = Arc::clone(&host);
            scope.spawn(move || host.ensure_indexed_ready(owner))
        };
        spin_until("leader 1 parked", || {
            flight_parked[0].load(Ordering::SeqCst)
        });
        // The follower joins leader 1's parked lane under an enclosing
        // cold-compute completeness scope (the admission-gate signal).
        let follower = {
            let host = Arc::clone(&host);
            scope.spawn(move || {
                let scope_guard = crate::request_context::ColdComputeCompletenessScope::enter();
                let result = host.ensure_indexed_ready(owner);
                let partial =
                    crate::request_context::current_cold_compute_completeness().is_partial();
                drop(scope_guard);
                (result, partial)
            })
        };
        spin_until("follower joined lane 1", || lane_count(&host) >= 3);
        mutate(&host, dep2);
        flight_release[0].store(true, Ordering::SeqCst);
        let _ = l1.join().unwrap();
        spin_until("follower parked after attempt 1", || {
            retry_parked[0].load(Ordering::SeqCst)
        });

        // Leader 2 claims a fresh lane; the follower re-joins it.
        let l2 = {
            let host = Arc::clone(&host);
            scope.spawn(move || host.ensure_indexed_ready(owner))
        };
        spin_until("leader 2 parked", || {
            flight_parked[1].load(Ordering::SeqCst)
        });
        mutate(&host, dep);
        retry_release[0].store(true, Ordering::SeqCst);
        spin_until("follower joined lane 2", || lane_count(&host) >= 3);
        flight_release[1].store(true, Ordering::SeqCst);
        let _ = l2.join().unwrap();
        spin_until("follower parked after attempt 2", || {
            retry_parked[1].load(Ordering::SeqCst)
        });

        // Leader 3: the follower's final bounded attempt.
        let l3 = {
            let host = Arc::clone(&host);
            scope.spawn(move || host.ensure_indexed_ready(owner))
        };
        spin_until("leader 3 parked", || {
            flight_parked[2].load(Ordering::SeqCst)
        });
        mutate(&host, dep2);
        retry_release[1].store(true, Ordering::SeqCst);
        spin_until("follower joined lane 3", || lane_count(&host) >= 3);
        flight_release[2].store(true, Ordering::SeqCst);
        let _ = l3.join().unwrap();

        follower.join().unwrap()
    });

    let follower_result =
        follower_result.expect("the bounded fallback must still serve the caller");
    assert!(
        follower_partial,
        "a ReturnOnly fallback serve must fold partiality into the \
         enclosing cold-compute scope so downstream admission is suppressed",
    );
    // ReturnOnly never publishes: every flight was fenced, so the store
    // must hold NO artifact for the owner.
    assert!(
        host.project_type_store().indexed().get_any(owner).is_none(),
        "no fenced flight may have published an artifact",
    );
    assert!(
        !host.indexed_surface_is_current(owner, &follower_result),
        "the served fallback is a known-superseded surface",
    );

    // Negative control: a clean serve folds NO partiality.
    *host.materialize_seam_hook.lock() = None;
    *host.flight_retry_seam_hook.lock() = None;
    let scope_guard = crate::request_context::ColdComputeCompletenessScope::enter();
    let clean = host.ensure_indexed_ready(owner);
    assert!(clean.is_some(), "the clean re-run must serve");
    assert!(
        !crate::request_context::current_cold_compute_completeness().is_partial(),
        "a published serve must not fold partiality",
    );
    drop(scope_guard);
}

/// `set_import_dependencies` mutates per-canonical route state without a
/// content change; it must be FENCE-VISIBLE. A flight that resolved its
/// routes against the pre-mutation table and reaches the pre-publish
/// fence after the mutation landed must publish NOTHING (ReturnOnly) —
/// otherwise a stale route surface is published that afterwards passes
/// `indexed_surface_is_current` and is served warm indefinitely.
#[test]
fn import_dependency_mutation_mid_flight_trips_the_publish_fence() {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    let host = make_host(&[]);
    let owner = "/workspace/src/owner.ts";
    let dep = "/workspace/src/dep.ts";
    let dep2 = "/workspace/src/dep2.ts";
    upsert(&host, dep, "export type P = { a: 1 };\n");
    upsert(&host, dep2, "export type P = { b: 2 };\n");
    upsert(
        &host,
        owner,
        "import type { P } from './dep';\nexport type Owner = P;\n",
    );
    host.provenance().reset();

    // Park the FIRST flight at its PRE-FENCE seam (the base materialise
    // fires the seam twice: post-stamp-capture = call 0, pre-fence =
    // call 1) — i.e. AFTER it resolved its route surface against the
    // pre-mutation table, BEFORE it can publish.
    let parked_pre_fence = Arc::new(AtomicBool::new(false));
    let release = Arc::new(AtomicBool::new(false));
    let seam_calls = Arc::new(AtomicUsize::new(0));
    {
        let parked_pre_fence = Arc::clone(&parked_pre_fence);
        let release = Arc::clone(&release);
        let seam_calls = Arc::clone(&seam_calls);
        *host.materialize_seam_hook.lock() = Some(Arc::new(move || {
            if seam_calls.fetch_add(1, Ordering::SeqCst) == 1 {
                parked_pre_fence.store(true, Ordering::SeqCst);
                spin_until("seam release", || release.load(Ordering::SeqCst));
            }
        }));
    }

    let stale_flight_result = std::thread::scope(|scope| {
        let flight = {
            let host = Arc::clone(&host);
            scope.spawn(move || host.ensure_indexed_ready(owner))
        };
        spin_until("flight parked pre-fence", || {
            parked_pre_fence.load(Ordering::SeqCst)
        });
        // The flight holds a fully-built route surface resolved against
        // the OLD table. Land the route mutation now.
        host.set_import_dependencies(
            owner,
            vec![crate::types::DependencyResolution {
                specifier: "./dep".to_string(),
                resolved_canonical_id: Some(dep2.to_string()),
                possible_canonical_ids: vec![dep2.to_string()],
            }],
        );
        release.store(true, Ordering::SeqCst);
        flight.join().unwrap().expect("flight must serve a result")
    });

    // ReturnOnly serving to the flight's own caller: the pre-mutation
    // route surface (sanity — proves the flight really raced).
    assert_eq!(
        stale_flight_result
            .import_routes
            .get("./dep")
            .and_then(|r| r.resolved_canonical_id.as_deref()),
        Some(dep),
        "the raced flight resolved against the pre-mutation table",
    );
    // The mutation must have made the flight's stamps stale.
    assert!(
        !host.indexed_surface_is_current(owner, &stale_flight_result),
        "a surface built against the superseded route table must not \
         pass the currency gate after the mutation",
    );
    // The discriminator: a fresh read must observe the POST-mutation
    // route table. Pre-fix the fenceless flight published its stale
    // surface, which then passed `indexed_surface_is_current` and was
    // served warm here.
    let fresh = host
        .ensure_indexed_ready(owner)
        .expect("post-mutation read must materialise");
    assert_eq!(
        fresh
            .import_routes
            .get("./dep")
            .and_then(|r| r.resolved_canonical_id.as_deref()),
        Some(dep2),
        "a post-mutation read must observe the post-mutation route table \
         — the raced flight must not have published its stale surface as \
         current",
    );
    assert!(
        host.indexed_surface_is_current(owner, &fresh),
        "the post-mutation read must serve a current surface",
    );
}

/// `set_import_dependencies` is called by the bundler after EVERY upsert
/// — the steady-state call re-supplies an IDENTICAL route snapshot. A
/// no-op call must NOT bump `project_generation` (a bump read-invalidates
/// every `validated_at_generation`-gated cache project-wide and stamps
/// every cross-file-edge `IndexedReady` stale); an actually-changing call
/// MUST bump (fence-visibility — see
/// `import_dependency_mutation_mid_flight_trips_the_publish_fence`).
#[test]
fn set_import_dependencies_bumps_only_on_actual_route_change() {
    let host = make_host(&[]);
    let owner = "/workspace/src/owner.ts";
    let dep = "/workspace/src/dep.ts";
    upsert(&host, dep, "export type P = { a: 1 };\n");
    upsert(
        &host,
        owner,
        "import type { P } from './dep';\nexport type Owner = P;\n",
    );
    let routes = vec![crate::types::DependencyResolution {
        specifier: "./dep".to_string(),
        resolved_canonical_id: Some(dep.to_string()),
        possible_canonical_ids: vec![dep.to_string()],
    }];

    // Changing call (empty table → routes): must be fence-visible.
    let pre = host.project_type_store().current_project_generation();
    host.set_import_dependencies(owner, routes.clone());
    let post_change = host.project_type_store().current_project_generation();
    assert!(
        post_change > pre,
        "an actually-changing route push must bump project_generation \
         (fence-visibility)",
    );

    // Materialise under the admitted table, then re-push the IDENTICAL
    // snapshot (the bundler steady-state).
    let _ = host
        .ensure_indexed_ready(owner)
        .expect("owner must materialise");
    host.provenance().reset();
    host.set_import_dependencies(owner, routes);
    let post_noop = host.project_type_store().current_project_generation();
    assert_eq!(
        post_noop, post_change,
        "a no-op route re-push must NOT bump project_generation \
         (project-wide warm-cache wipe per bundler push)",
    );

    // Warm state intact: the retained artifact is still current — the
    // next read serves it without an edge refresh or any rebuild.
    let again = host
        .ensure_indexed_ready(owner)
        .expect("post-no-op read must serve");
    assert!(
        host.indexed_surface_is_current(owner, &again),
        "the retained artifact must stay current across a no-op re-push",
    );
    let provenance = snap(&host);
    assert_eq!(
        provenance.indexed_ready_edge_refreshes, 0,
        "a no-op re-push must not force an edge refresh"
    );
    assert_eq!(
        provenance.eval_program_parses, 0,
        "a no-op re-push must not force a re-parse"
    );
    assert_eq!(
        provenance.indexed_ready_materializes, 0,
        "a no-op re-push must not force a re-materialise"
    );
}

/// Overlay variant of the pre-publish fence: the overlay materialiser
/// publishes into `FileArtifactStore` and must carry the SAME fence
/// semantics as the base materialise + edge refresh — a generation move
/// mid-flight serves the result to the caller (ReturnOnly) and publishes
/// NOTHING.
#[test]
fn overlay_mid_flight_mutation_trips_the_overlay_publish_fence() {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use crate::session_view::OverlaidView;

    let host = make_host(&[]);
    let canonical = "/workspace/src/overlaid_fence.ts";
    let other = "/workspace/src/other.ts";
    upsert(&host, canonical, "export type Base = { a: 1 };\n");
    upsert(&host, other, "export type Other = { o: 1 };\n");

    let mut overlays: rustc_hash::FxHashMap<String, Arc<str>> = rustc_hash::FxHashMap::default();
    overlays.insert(
        canonical.to_string(),
        Arc::from("export type Overlaid = { b: 2 };\n"),
    );
    let view = OverlaidView::new(Arc::clone(&host), overlays);
    host.provenance().reset();

    // Park the overlay flight at its PRE-FENCE seam (the overlay
    // materialise fires the seam twice: post-stamp-capture = call 0,
    // pre-fence = call 1).
    let parked_pre_fence = Arc::new(AtomicBool::new(false));
    let release = Arc::new(AtomicBool::new(false));
    let seam_calls = Arc::new(AtomicUsize::new(0));
    {
        let parked_pre_fence = Arc::clone(&parked_pre_fence);
        let release = Arc::clone(&release);
        let seam_calls = Arc::clone(&seam_calls);
        *host.materialize_seam_hook.lock() = Some(Arc::new(move || {
            if seam_calls.fetch_add(1, Ordering::SeqCst) == 1 {
                parked_pre_fence.store(true, Ordering::SeqCst);
                spin_until("seam release", || release.load(Ordering::SeqCst));
            }
        }));
    }

    let fenced = std::thread::scope(|scope| {
        let flight = {
            let host = Arc::clone(&host);
            let view = &view;
            scope.spawn(move || host.materialize_overlay_indexed_ready_with_view(canonical, view))
        };
        spin_until("flight parked pre-fence", || {
            parked_pre_fence.load(Ordering::SeqCst)
        });
        // Land a route-resolution mutation while the overlay flight holds
        // its fully-built artifact, pre-publish. A GENUINE change is
        // required: the changed-gate skips the cascade (and the
        // generation bump) for a value-identical push, so an empty push
        // onto an empty table would no longer trip the fence.
        host.set_exact_resolutions(
            other,
            vec![verter_workspace::ExactResolution {
                specifier: "./fence_probe".to_string(),
                phase: verter_workspace::ResolvePhase::CodegenBlocker,
                kind: verter_workspace::ResolveRequestKind::TypeImport,
                resolved_canonical_id: Some(canonical.to_string()),
                possible_canonical_ids: vec![canonical.to_string()],
            }],
        );
        release.store(true, Ordering::SeqCst);
        flight
            .join()
            .unwrap()
            .expect("the fenced overlay flight must still serve its caller")
    });

    // ReturnOnly serves the caller…
    assert!(
        fenced.shallow_state.symbol("Overlaid").is_some(),
        "the fenced overlay artifact must still carry the overlay surface",
    );
    // …but publishes NOTHING.
    assert!(
        host.overlay_artifact_identity(canonical)
            .lookup_overlay_artifacts(&host, &view)
            .is_none(),
        "a generation move mid-flight must keep the overlay artifact OUT \
         of FileArtifactStore (ReturnOnly never publishes)",
    );

    // The next overlay read re-materialises against live state and
    // publishes a current candidate.
    *host.materialize_seam_hook.lock() = None;
    let fresh = host
        .materialize_overlay_indexed_ready_with_view(canonical, &view)
        .expect("post-mutation overlay read must materialise");
    assert!(
        host.indexed_surface_is_current(canonical, &fresh),
        "the re-materialised overlay artifact must be current",
    );
    assert!(
        host.overlay_artifact_identity(canonical)
            .lookup_overlay_artifacts(&host, &view)
            .is_some(),
        "the unfenced re-materialise must publish the overlay candidate",
    );
    assert_eq!(
        snap(&host).indexed_ready_materializes,
        2,
        "fenced flight + re-materialise = exactly two overlay builds",
    );
}

/// Overlay materialise singleflight: concurrent overlay requests on ONE
/// session view (the realistic batch rayon shape — one shared view) must
/// collapse onto a single cold build, per the Canonical-Dependency-Cache
/// collapse contract. Deterministic schedule: the leader parks at its
/// post-stamp seam; three followers join the overlay lane; release; all
/// four serve the same published Arc and exactly one build ran.
#[test]
fn concurrent_overlay_materialise_collapses() {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use crate::session_view::{OverlaidView, SessionView};

    let host = make_host(&[]);
    let canonical = "/workspace/src/overlaid_concurrent.ts";
    upsert(&host, canonical, "export type Base = { a: 1 };\n");
    let mut overlays: rustc_hash::FxHashMap<String, Arc<str>> = rustc_hash::FxHashMap::default();
    overlays.insert(
        canonical.to_string(),
        Arc::from("export type Overlaid = { b: 2 };\n"),
    );
    let view = OverlaidView::new(Arc::clone(&host), overlays);
    host.provenance().reset();

    // Park ONLY the first flight at its post-stamp seam (call 0) so the
    // followers' claims are concurrent with the leader's build.
    let leader_parked = Arc::new(AtomicBool::new(false));
    let release = Arc::new(AtomicBool::new(false));
    let seam_calls = Arc::new(AtomicUsize::new(0));
    {
        let leader_parked = Arc::clone(&leader_parked);
        let release = Arc::clone(&release);
        let seam_calls = Arc::clone(&seam_calls);
        *host.materialize_seam_hook.lock() = Some(Arc::new(move || {
            if seam_calls.fetch_add(1, Ordering::SeqCst) == 0 {
                leader_parked.store(true, Ordering::SeqCst);
                spin_until("seam release", || release.load(Ordering::SeqCst));
            }
        }));
    }

    // The production overlay lane identity (canonical + overlay content
    // hash + overlay-set discriminator).
    let lane_hash = view
        .content_hash_for(canonical)
        .expect("the overlaid view reports a content hash");
    let lane_discriminator = view.overlay_artifact_discriminator(canonical);
    let lane_key =
        format!("overlay\u{0}{canonical}\u{0}{lane_hash:02x?}\u{0}{lane_discriminator:02x?}");
    let token = crate::resolver_core::StoreViewCompatToken {
        epoch: 0,
        session: None,
        validity_fingerprint: 0,
    };

    let results: Vec<Arc<crate::project_type_store::IndexedReady>> = std::thread::scope(|scope| {
        let leader = {
            let host = Arc::clone(&host);
            let view = &view;
            scope.spawn(move || {
                host.materialize_overlay_indexed_ready_with_view(canonical, view)
                    .expect("leader overlay materialise must serve")
            })
        };
        spin_until("leader parked", || leader_parked.load(Ordering::SeqCst));
        let followers: Vec<_> = (0..3)
            .map(|_| {
                let host = Arc::clone(&host);
                let view = &view;
                scope.spawn(move || {
                    host.materialize_overlay_indexed_ready_with_view(canonical, view)
                        .expect("follower overlay materialise must serve")
                })
            })
            .collect();
        // Post-fix: the followers JOIN the leader's overlay lane
        // (strong count grows past the leader-only baseline of 2).
        // Pre-fix there is no lane: each follower runs its own full
        // build, firing its own seam calls — terminate the wait on
        // either signal so a regression FAILS (on the build-count
        // assert) instead of hanging.
        spin_until("followers joined the overlay lane (or ran solo)", || {
            host.resolver
                .runtime
                .indexed_singleflight
                .test_flight_strong_count(&lane_key, token)
                >= 5
                || seam_calls.load(Ordering::SeqCst) >= 7
        });
        release.store(true, Ordering::SeqCst);
        let mut results = vec![leader.join().unwrap()];
        results.extend(followers.into_iter().map(|h| h.join().unwrap()));
        results
    });

    let first = &results[0];
    for other in &results[1..] {
        assert!(
            Arc::ptr_eq(first, other),
            "the overlay singleflight must hand every concurrent caller \
             the same published Arc"
        );
    }
    assert!(
        first.shallow_state.symbol("Overlaid").is_some(),
        "anti-vacuity: the served artifact carries the overlay surface",
    );
    let provenance = snap(&host);
    assert_eq!(
        provenance.indexed_ready_materializes, 1,
        "concurrent overlay requests on one view must collapse onto \
         exactly ONE cold build (got {})",
        provenance.indexed_ready_materializes,
    );
    assert_eq!(
        provenance.eval_program_parses, 1,
        "exactly one eval-program parse across the collapsed builds"
    );
}

/// Fact capture must OBSERVE, never build: `current_derived_fact_hash`
/// (the producer-side capture `append_dependency_fact_versions` runs for
/// every dependency) must never materialise, edge-refresh, or publish —
/// for a never-materialised canonical the more-sensitive `FileWholeHash`
/// fact covers invalidation; for a stale surface the capture declines
/// (`None`) so dependents fail warm validation and recompute themselves.
#[test]
fn route_fact_capture_is_side_effect_free() {
    let assert_no_builds = |provenance: &MetaProvenanceSnapshot, label: &str| {
        assert_eq!(
            provenance.indexed_ready_materializes, 0,
            "{label}: fact capture must not materialise",
        );
        assert_eq!(
            provenance.indexed_ready_edge_refreshes, 0,
            "{label}: fact capture must not edge-refresh",
        );
        assert_eq!(
            provenance.eval_program_parses, 0,
            "{label}: fact capture must not parse",
        );
    };

    let host = make_host(&[]);
    let owner = "/workspace/src/owner.ts";
    let dep = "/workspace/src/dep.ts";
    let dep2 = "/workspace/src/dep2.ts";
    upsert(&host, dep, "export type P = { a: 1 };\n");
    upsert(&host, dep2, "export type P = { b: 2 };\n");
    upsert(
        &host,
        owner,
        "import type { P } from './dep';\nexport type Owner = P;\n",
    );
    host.provenance().reset();

    // 1. NEVER-MATERIALISED canonical: capture observes nothing, builds
    //    nothing.
    let captured =
        host.current_derived_fact_hash(owner, crate::resolver_core::DerivedFactKind::Route);
    assert!(
        captured.is_none(),
        "a never-materialised canonical has no Route fact (FileWholeHash \
         covers it)",
    );
    assert_no_builds(&snap(&host), "never-materialised capture");

    // 2. STALE surface (route mutation moved the project stamp): capture
    //    declines instead of refreshing.
    let _ = host
        .ensure_indexed_ready(owner)
        .expect("owner must materialise");
    host.set_exact_resolutions(
        owner,
        vec![verter_workspace::ExactResolution {
            specifier: "./dep".to_string(),
            phase: verter_workspace::ResolvePhase::CodegenBlocker,
            kind: verter_workspace::ResolveRequestKind::TypeImport,
            resolved_canonical_id: Some(dep2.to_string()),
            possible_canonical_ids: vec![dep2.to_string()],
        }],
    );
    host.provenance().reset();
    let stale_capture =
        host.current_derived_fact_hash(owner, crate::resolver_core::DerivedFactKind::Route);
    assert!(
        stale_capture.is_none(),
        "a stale surface's capture must decline (None), never refresh it",
    );
    assert_no_builds(&snap(&host), "stale-surface capture");

    // 3. CONTENT-STALE candidate (owner edited, never re-materialised):
    //    capture declines, builds nothing.
    upsert(
        &host,
        owner,
        "import type { P } from './dep';\nexport type Owner = { p: P };\n",
    );
    host.provenance().reset();
    let content_stale_capture =
        host.current_derived_fact_hash(owner, crate::resolver_core::DerivedFactKind::Route);
    assert!(
        content_stale_capture.is_none(),
        "a content-stale candidate's capture must decline (None), never \
         rebuild the surface",
    );
    assert_no_builds(&snap(&host), "content-stale capture");

    // 4. CURRENT surface: capture observes the stored hash, still builds
    //    nothing.
    let current = host
        .ensure_indexed_ready(owner)
        .expect("owner must re-materialise");
    host.provenance().reset();
    let current_capture =
        host.current_derived_fact_hash(owner, crate::resolver_core::DerivedFactKind::Route);
    assert_eq!(
        current_capture, current.route_hash,
        "a current surface's capture observes the stored route_hash",
    );
    assert!(
        current_capture.is_some(),
        "the owner has a resolvable surface, so the Route fact must exist",
    );
    assert_no_builds(&snap(&host), "current-surface capture");
}

/// Route-fact producer/validator parity: the producer-side
/// `current_route_surface_hash` and the `HostStoreView` validator
/// snapshot must derive the `Route` fact from the SAME (IndexedReady)
/// source for every canonical a cold resolve materialised.
#[test]
fn route_fact_producer_matches_validator_snapshot() {
    let host = make_host(&[]);
    let barrel = "/workspace/src/barrel.ts";
    let leaf = "/workspace/src/leaf.ts";
    upsert(&host, leaf, "export type Props = { label: string };\n");
    upsert(&host, barrel, "export type { Props } from './leaf';\n");

    let node = host.resolve_named_symbol(barrel, "Props", &[], Some(ProjectionMode::Expanded));
    assert!(node.is_some(), "Props must resolve through the barrel");

    let view = host.resolver_store_view_read().into_owned_view();
    for canonical in [barrel, leaf] {
        let producer = host.current_route_surface_hash(canonical);
        let validator = crate::resolver_core::StoreView::derived_hash_for(
            &view,
            canonical,
            crate::resolver_core::DerivedFactKind::Route,
        );
        // Both files were materialised by the resolve and carry a
        // resolvable surface, so the Route fact MUST exist on both
        // sides — a `None == None` agreement would be vacuous.
        assert!(
            producer.is_some(),
            "the producer must record a Route fact for the materialised \
             {canonical} — None == None agreement is vacuous"
        );
        assert_eq!(
            producer, validator,
            "producer route hash and validator snapshot must agree for \
             {canonical} (producer={producer:?}, validator={validator:?})"
        );
    }
}

/// The `None`-side parity sub-invariant: a CURRENT `IndexedReady` whose
/// surface is NOT route-resolvable (no symbols / exports / reexports)
/// produces NO `Route` fact on the producer side AND contributes no
/// `Route` derived hash to the validator snapshot — neither side may
/// manufacture a hash the other suppresses (a lingering one-sided hash
/// is exactly how a stale dependent entry keeps validating).
#[test]
fn route_fact_none_for_non_route_resolvable_current_surface() {
    let host = make_host(&[]);
    let plain = "/workspace/src/effects_only.ts";
    // No exports, no type/value symbols, no reexports: nothing
    // route-resolvable on the surface.
    upsert(&host, plain, "console.log(1);\n");

    let indexed = host
        .ensure_indexed_ready(plain)
        .expect("the canonical must materialise");
    // Anti-vacuity: the artifact is genuinely CURRENT and genuinely
    // non-route-resolvable.
    assert!(
        host.indexed_surface_is_current(plain, &indexed),
        "precondition: the materialised artifact is current",
    );
    assert!(
        !indexed.shallow_state.has_resolvable_surface(),
        "precondition: the fixture surface must not be route-resolvable",
    );

    let producer = host.current_route_surface_hash(plain);
    assert!(
        producer.is_none(),
        "producer: no Route fact for a non-route-resolvable surface",
    );
    let view = host.resolver_store_view_read().into_owned_view();
    // The view tracks the canonical (whole hash present) …
    assert!(
        view.whole_hash(plain).is_some(),
        "anti-vacuity: the validator view must track the materialised \
         canonical",
    );
    // … but carries no Route derived hash for it.
    let validator = crate::resolver_core::StoreView::derived_hash_for(
        &view,
        plain,
        crate::resolver_core::DerivedFactKind::Route,
    );
    assert!(
        validator.is_none(),
        "validator: no Route derived hash for a non-route-resolvable \
         surface (got {validator:?})",
    );
}

/// The never-materialised None case: a canonical the indexed store has
/// never built has NO `Route` fact — and the fact-capture read
/// (`current_derived_fact_hash(Route)`, the observe-only rail) must
/// DECLINE rather than materialise the canonical just to sign a
/// result. A capture that cold-builds here breadth-walks every
/// unrelated import of the owner (the exact violation the
/// `macro_surface_no_breadth_walk` guard pins); the canonical's
/// `FileWholeHash` fact alone roots invalidation on its own content,
/// and content-bypassing route mutations are covered by the
/// `project_generation` fence + fact.
#[test]
fn route_fact_capture_declines_for_never_materialised_canonical() {
    let host = make_host(&[]);
    let never = "/workspace/src/never_materialised.ts";
    upsert(&host, never, "export type N = { a: 1 };\n");
    // Deliberately NO ensure_indexed_ready / resolve for this canonical.
    assert!(
        host.project_type_store().indexed().get_any(never).is_none(),
        "precondition: the canonical has never been materialised",
    );

    let captured =
        host.current_derived_fact_hash(never, crate::resolver_core::DerivedFactKind::Route);
    assert_eq!(
        captured, None,
        "fact capture must record NO Route fact for a never-materialised \
         canonical (its first traversal materialises it; capture observes)",
    );
    assert!(
        host.project_type_store().indexed().get_any(never).is_none(),
        "the capture read must be OBSERVE-ONLY — it must not have \
         materialised the canonical as a side effect",
    );
}

/// Stamp-only route-mutation pin: a ROUTE-resolution
/// mutation must not flip a scheduler-tracked canonical into the
/// artifact-only class. Pre-fix, `configure_projects` /
/// `set_exact_resolutions` ran the authority-reset wide evict
/// (`bump_project_generation_and_evict`), which wholesale-cleared
/// `derived_raw_cache` — and the artifact-only oracle, which read ONLY
/// entry absence, then classified every scheduler-tracked canonical as
/// artifact-only and served it through the permissive `get_any` lanes.
/// Two independent fixes both pin here: the route mutators no longer
/// wide-clear, and the oracle consults the scheduler source as well.
#[test]
fn scheduler_tracked_canonical_never_turns_artifact_only_on_route_mutation() {
    let host = make_host(&[]);
    upsert(&host, SCRATCH_ID, SCRATCH);
    host.ensure_indexed_ready(SCRATCH_ID)
        .expect("cold materialise");
    assert!(
        !host.is_artifact_only_scope(SCRATCH_ID),
        "precondition: an upserted canonical is scheduler-tracked",
    );

    // Route mutation 1: a project reconfigure.
    host.configure_projects(vec![
        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.json".to_string()),
        ),
    ]);
    assert!(
        !host.is_artifact_only_scope(SCRATCH_ID),
        "configure_projects must not flip a scheduler-tracked canonical \
         into the artifact-only class (stamp-only route freshness; \
         payloads retained, stale entries miss by validation)",
    );
    // The per-canonical derived entry SURVIVES the route mutation (only
    // its route-mirror fields were cleared) — retained payloads are the
    // invariant's core.
    assert!(
        host.derived_raw_cache().get(SCRATCH_ID).is_some(),
        "the DerivedRawState entry must survive a project reconfigure \
         (wide per-canonical clears are reserved for authority resets)",
    );

    // Route mutation 2: an exact-resolution push.
    host.set_exact_resolutions(
        SCRATCH_ID,
        vec![verter_workspace::ExactResolution {
            specifier: "./somewhere".to_string(),
            phase: verter_workspace::ResolvePhase::CodegenBlocker,
            kind: verter_workspace::ResolveRequestKind::TypeImport,
            resolved_canonical_id: Some("/workspace/src/somewhere.ts".to_string()),
            possible_canonical_ids: vec!["/workspace/src/somewhere.ts".to_string()],
        }],
    );
    assert!(
        !host.is_artifact_only_scope(SCRATCH_ID),
        "set_exact_resolutions must not flip a scheduler-tracked \
         canonical into the artifact-only class",
    );

    // Oracle hardening: even with the derived entry REMOVED outright
    // (any wide per-canonical clear), a PRESENT scheduler source keeps
    // the scheduler the content authority.
    host.derived_raw_cache().remove(SCRATCH_ID);
    assert!(
        !host.is_artifact_only_scope(SCRATCH_ID),
        "a scheduler source present means the scheduler is the content \
         authority even with no DerivedRawState entry — entry absence \
         alone is not scheduler-untrackedness",
    );
}

/// Owner-scoped repair pin (`set_exact_resolutions`): a
/// REPLACED exact resolution must stop serving the old target — the
/// owner's derived route mirror is cleared and the next resolve
/// observes the new exact table; and a VALUE-IDENTICAL re-push is a
/// true no-op (no project-generation bump — the changed-gate skips the
/// cascade).
#[test]
fn set_exact_resolutions_replacement_reroutes_and_identical_repush_is_noop() {
    let host = make_host(&[]);
    let owner = "/workspace/src/owner.ts";
    let dep1 = "/workspace/src/dep1.ts";
    let dep2 = "/workspace/src/dep2.ts";
    upsert(&host, dep1, "export type P = { a: 1 };\n");
    upsert(&host, dep2, "export type P = { b: 2 };\n");
    upsert(
        &host,
        owner,
        "import type { P } from './dep';\nexport type Owner = P;\n",
    );

    let exact = |target: &str| {
        vec![verter_workspace::ExactResolution {
            specifier: "./dep".to_string(),
            phase: verter_workspace::ResolvePhase::CodegenBlocker,
            kind: verter_workspace::ResolveRequestKind::TypeImport,
            resolved_canonical_id: Some(target.to_string()),
            possible_canonical_ids: vec![target.to_string()],
        }]
    };

    host.set_exact_resolutions(owner, exact(dep1));
    let _ = host.ensure_indexed_ready(owner);
    let first = host
        .ensure_indexed_ready(owner)
        .expect("owner materialises")
        .import_routes
        .get("./dep")
        .and_then(|r| r.resolved_canonical_id.clone());
    assert_eq!(
        first.as_deref(),
        Some(dep1),
        "precondition: the first exact table routes ./dep to dep1",
    );

    // REPLACEMENT: the old exact must stop serving.
    host.set_exact_resolutions(owner, exact(dep2));
    let second = host
        .ensure_indexed_ready(owner)
        .expect("owner re-serves")
        .import_routes
        .get("./dep")
        .and_then(|r| r.resolved_canonical_id.clone());
    assert_eq!(
        second.as_deref(),
        Some(dep2),
        "a replaced exact resolution must reroute — the owner-scoped \
         route-mirror clear plus the stamp gate must surface the new \
         exact table on the next read",
    );

    // VALUE-IDENTICAL re-push: true no-op, no generation bump.
    let generation = host.project_type_store.current_project_generation();
    host.set_exact_resolutions(owner, exact(dep2));
    assert_eq!(
        host.project_type_store.current_project_generation(),
        generation,
        "a value-identical exact re-push must not bump project_generation \
         (the changed-gate skips the cascade — steady-state bundler \
         re-pushes stay free)",
    );
}

/// Install a `materialize_seam_hook` that parks the NEXT `IndexedReady`
/// materialise flight at its PRE-FENCE seam (the base materialise fires
/// the seam twice: post-stamp-capture = call 0, pre-fence = call 1) —
/// AFTER the artifact is fully built against the pre-mutation state,
/// BEFORE the fence can observe a mid-flight mutation. Later flights
/// (seam calls ≥ 2) pass through unparked. Returns `(parked, release)`:
/// spin on `parked`, land the mutation, then store `release` to let the
/// flight run into its fence.
fn park_first_materialize_pre_fence(
    host: &VerterHost,
) -> (
    Arc<std::sync::atomic::AtomicBool>,
    Arc<std::sync::atomic::AtomicBool>,
) {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    let parked_pre_fence = Arc::new(AtomicBool::new(false));
    let release = Arc::new(AtomicBool::new(false));
    let seam_calls = Arc::new(AtomicUsize::new(0));
    {
        let parked_pre_fence = Arc::clone(&parked_pre_fence);
        let release = Arc::clone(&release);
        *host.materialize_seam_hook.lock() = Some(Arc::new(move || {
            if seam_calls.fetch_add(1, Ordering::SeqCst) == 1 {
                parked_pre_fence.store(true, Ordering::SeqCst);
                spin_until("seam release", || release.load(Ordering::SeqCst));
            }
        }));
    }
    (parked_pre_fence, release)
}

/// The route-resolution mutation the fence tests land while a flight is
/// parked pre-fence: an exact-resolution push on an UNRELATED canonical
/// bumps `project_generation`, so the parked flight's fence trips while
/// the flight owner's own facts stay live-valid — the poison shape the
/// read-side fact rail cannot catch.
fn land_unrelated_route_mutation(host: &VerterHost, other: &str, target: &str) {
    host.set_exact_resolutions(
        other,
        vec![verter_workspace::ExactResolution {
            specifier: "./fence_probe".to_string(),
            phase: verter_workspace::ResolvePhase::CodegenBlocker,
            kind: verter_workspace::ResolveRequestKind::TypeImport,
            resolved_canonical_id: Some(target.to_string()),
            possible_canonical_ids: vec![target.to_string()],
        }],
    );
}

/// ReturnOnly never publishes — prepared-decl-bundle arm. A bundle built
/// FROM a fenced (served-without-publication) `IndexedReady` must not be
/// admitted into the shared `prepared_decl_bundles` cache. The fenced
/// artifact's route surface was resolved against superseded state, while
/// the bundle's fact stamps (owner whole-hash; ImportRoute hash via
/// `generation_current_import_route_hash`) are computed from the LIVE
/// post-mutation state — so the recorded facts genuinely match a fresh
/// view and the read-side fact rail cannot reject the entry. Admission
/// itself must decline; the caller is still served (its request
/// pre-dates the mutation).
#[test]
fn bundle_built_from_fenced_indexed_ready_is_served_but_not_admitted() {
    use std::sync::atomic::Ordering;

    let host = make_host(&[]);
    let owner = "/workspace/src/owner.ts";
    let dep = "/workspace/src/dep.ts";
    let other = "/workspace/src/other.ts";
    upsert(&host, dep, "export type P = { a: 1 };\n");
    upsert(&host, other, "export type Other = { o: 1 };\n");
    upsert(
        &host,
        owner,
        "import type { P } from './dep';\nexport type Owner = P;\n",
    );
    host.provenance().reset();

    // Park the owner's IndexedReady flight pre-fence — AFTER the route
    // surface resolved against the pre-mutation state, BEFORE it can
    // publish.
    let (parked_pre_fence, release) = park_first_materialize_pre_fence(&host);

    let bundle = std::thread::scope(|scope| {
        let flight = {
            let host = Arc::clone(&host);
            scope.spawn(move || host.prepared_decl_bundle(owner))
        };
        spin_until("flight parked pre-fence", || {
            parked_pre_fence.load(Ordering::SeqCst)
        });
        // Land a route-resolution mutation (project_generation bump)
        // while the IndexedReady flight holds its fully-built artifact
        // pre-publish.
        land_unrelated_route_mutation(&host, other, dep);
        release.store(true, Ordering::SeqCst);
        flight.join().unwrap()
    });
    assert!(
        bundle.is_some(),
        "the fenced flight must still serve its own caller a bundle",
    );
    assert_eq!(
        snap(&host).bundle_materializations,
        1,
        "sanity: the first read cold-built the bundle",
    );

    // The discriminator: the fenced-rooted bundle must NOT have been
    // admitted — a fresh read cold-rebuilds against live state instead
    // of warm-hitting an entry whose facts validate but whose payload
    // was computed from the superseded route surface.
    *host.materialize_seam_hook.lock() = None;
    host.provenance().reset();
    let second = host.prepared_decl_bundle(owner);
    assert!(second.is_some(), "the post-mutation read must serve");
    let provenance = snap(&host);
    assert_eq!(
        provenance.bundle_cache_hits, 0,
        "a bundle rooted at a fenced (ReturnOnly) IndexedReady must not \
         be admitted warm — the post-mutation read must not warm-hit it \
         (got {} warm hits)",
        provenance.bundle_cache_hits,
    );
    assert_eq!(
        provenance.bundle_materializations, 1,
        "the post-mutation read cold-rebuilds against live state",
    );

    // The gate must not over-decline: the unfenced rebuild above WAS
    // store-published, so a third read serves it warm.
    host.provenance().reset();
    let third = host.prepared_decl_bundle(owner);
    assert!(third.is_some(), "the warm read must serve");
    let provenance = snap(&host);
    assert_eq!(
        provenance.bundle_cache_hits, 1,
        "a bundle rooted at a store-published IndexedReady is admitted \
         and served warm",
    );
    assert_eq!(
        provenance.bundle_materializations, 0,
        "the warm read must not rebuild",
    );
}

/// ReturnOnly never publishes — ROUTED-SHALLOW prepared-decl-bundle arm
/// (the declaration-file sibling of
/// `bundle_built_from_fenced_indexed_ready_is_served_but_not_admitted`).
/// The routed-shallow producer
/// (`materialize_prepared_decl_bundle_from_routed_shallow`, declaration
/// files only) reads its state through the frontier route reader, whose
/// cold fall-through joins the same `ensure_indexed_ready` flight — so a
/// FENCED (ReturnOnly) `IndexedReady` can feed its bundle build. The
/// publication status must flow BY VALUE through that reader and gate
/// the shared-cache insert exactly like the standard producer: the
/// fenced flight's caller is still served its bundle, but the entry —
/// whose fact stamps (`generation_current_import_route_hash`) are read
/// from the LIVE post-mutation state while its payload was computed
/// from the superseded route surface — must never go warm.
#[test]
fn routed_shallow_bundle_built_from_fenced_indexed_ready_is_served_but_not_admitted() {
    use std::sync::atomic::Ordering;

    let host = make_host(&[]);
    // A `.d.ts` owner routes the bundle cold build through the
    // routed-shallow producer (the standard producer is its
    // `.or_else` fallback and must not run for this fixture).
    let owner = "/workspace/src/owner.d.ts";
    let dep = "/workspace/src/dep.ts";
    let other = "/workspace/src/other.ts";
    upsert(&host, dep, "export type P = { a: 1 };\n");
    upsert(&host, other, "export type Other = { o: 1 };\n");
    upsert(
        &host,
        owner,
        "import type { P } from './dep';\nexport type Owner = P;\n",
    );
    host.provenance().reset();

    // Park the owner's IndexedReady flight pre-fence; the routed-shallow
    // producer's cold state read joins this flight first, so seam call 1
    // is the owner's pre-fence point.
    let (parked_pre_fence, release) = park_first_materialize_pre_fence(&host);

    let bundle = std::thread::scope(|scope| {
        let flight = {
            let host = Arc::clone(&host);
            scope.spawn(move || host.prepared_decl_bundle(owner))
        };
        spin_until("flight parked pre-fence", || {
            parked_pre_fence.load(Ordering::SeqCst)
        });
        land_unrelated_route_mutation(&host, other, dep);
        release.store(true, Ordering::SeqCst);
        flight.join().unwrap()
    });
    assert!(
        bundle.is_some(),
        "the fenced flight must still serve its own caller a bundle",
    );
    assert_eq!(
        snap(&host).bundle_materializations,
        1,
        "sanity: the first read cold-built the bundle through the \
         routed-shallow producer",
    );

    // The discriminator: the fenced-rooted bundle must NOT have been
    // admitted — a fresh read cold-rebuilds against live state instead
    // of warm-hitting an entry whose facts validate but whose payload
    // was computed from the superseded route surface.
    *host.materialize_seam_hook.lock() = None;
    host.provenance().reset();
    let second = host.prepared_decl_bundle(owner);
    assert!(second.is_some(), "the post-mutation read must serve");
    let provenance = snap(&host);
    assert_eq!(
        provenance.bundle_cache_hits, 0,
        "a routed-shallow bundle rooted at a fenced (ReturnOnly) \
         IndexedReady must not be admitted warm — the post-mutation read \
         must not warm-hit it (got {} warm hits)",
        provenance.bundle_cache_hits,
    );
    assert_eq!(
        provenance.bundle_materializations, 1,
        "the post-mutation read cold-rebuilds against live state",
    );

    // The gate must not over-decline: the unfenced rebuild above WAS
    // store-published, so a third read serves it warm.
    host.provenance().reset();
    let third = host.prepared_decl_bundle(owner);
    assert!(third.is_some(), "the warm read must serve");
    let provenance = snap(&host);
    assert_eq!(
        provenance.bundle_cache_hits, 1,
        "a routed-shallow bundle rooted at a store-published IndexedReady \
         is admitted and served warm",
    );
    assert_eq!(
        provenance.bundle_materializations, 0,
        "the warm read must not rebuild",
    );
}

/// ReturnOnly never publishes — imported-root fast-path arm. The
/// direct-reexport fast path
/// (`resolve_direct_imported_type_root_fast_path`) reads the provider
/// and target shallow states through the routed reader; a FENCED serve
/// carries baked reexport edges resolved against the superseded route
/// table, while the fact list the fast path returns is read from the
/// LIVE post-mutation state — the unrejectable poison shape. A fenced
/// participant must route the result through the strict-admission
/// negative-cache pattern (EMPTY facts: served to the caller, never
/// persisted in `ImportedRootDb`).
#[test]
fn imported_root_fast_path_from_fenced_state_is_served_but_not_admitted() {
    use std::sync::atomic::Ordering;

    let host = make_host(&[]);
    let barrel = "/workspace/src/index.ts";
    let leaf = "/workspace/src/p.ts";
    let other = "/workspace/src/other.ts";
    upsert(&host, leaf, "export type P = { a: 1 };\n");
    upsert(&host, other, "export type Other = { o: 1 };\n");
    upsert(&host, barrel, "export type { P } from './p';\n");
    host.provenance().reset();

    // Park the barrel's IndexedReady flight pre-fence — the fast path's
    // provider read joins it first (the leaf's later flight passes the
    // seam unparked).
    let (parked_pre_fence, release) = park_first_materialize_pre_fence(&host);

    let (resolved, _facts) = std::thread::scope(|scope| {
        let flight = {
            let host = Arc::clone(&host);
            scope.spawn(move || host.resolve_imported_type_root_with_facts(barrel, "P"))
        };
        spin_until("flight parked pre-fence", || {
            parked_pre_fence.load(Ordering::SeqCst)
        });
        land_unrelated_route_mutation(&host, other, leaf);
        release.store(true, Ordering::SeqCst);
        flight.join().unwrap()
    });
    *host.materialize_seam_hook.lock() = None;
    assert_eq!(
        (resolved.0.as_str(), resolved.1.as_str()),
        (leaf, "P"),
        "the fenced resolve must still serve its caller the resolved root",
    );

    // The discriminator: the fenced-rooted result must NOT have been
    // admitted into the shared ImportedRootDb.
    assert!(
        host.resolver
            .runtime
            .imported_roots
            .get_any(barrel, "P")
            .is_none(),
        "an imported root resolved through a fenced (ReturnOnly) shallow \
         serve must not be admitted — its baked reexport edge was resolved \
         against the superseded route table while its recorded facts \
         validate against the live view",
    );

    // No over-decline: an unfenced re-resolve admits and serves the same
    // root.
    let (resolved, _facts) = host.resolve_imported_type_root_with_facts(barrel, "P");
    assert_eq!(
        (resolved.0.as_str(), resolved.1.as_str()),
        (leaf, "P"),
        "the unfenced re-resolve serves the same root",
    );
    assert!(
        host.resolver
            .runtime
            .imported_roots
            .get_any(barrel, "P")
            .is_some(),
        "an imported root resolved from store-published serves is admitted",
    );
}

/// ReturnOnly never publishes — route-entry (frontier walk) arm. The
/// barrel walk behind `build_named_type_export_route_entry` reads every
/// participant through the frontier route reader; a FENCED participant
/// serve means the produced route was computed from a superseded route
/// surface while the participant fact list is read from the LIVE state.
/// The entry must flow through the established strict-admission
/// negative-cache pattern (EMPTY facts — the same rail the
/// unproduce-able wildcard-hash case uses): the route result is served,
/// `RouteDb` never persists it.
#[test]
fn route_entry_built_from_fenced_participant_serves_with_empty_facts() {
    use std::sync::atomic::Ordering;

    let host = make_host(&[]);
    let barrel = "/workspace/src/index.ts";
    let leaf = "/workspace/src/p.ts";
    let other = "/workspace/src/other.ts";
    upsert(&host, leaf, "export type P = { a: 1 };\n");
    upsert(&host, other, "export type Other = { o: 1 };\n");
    upsert(&host, barrel, "export type { P } from './p';\n");
    host.provenance().reset();

    let (parked_pre_fence, release) = park_first_materialize_pre_fence(&host);

    let entry = std::thread::scope(|scope| {
        let flight = {
            let host = Arc::clone(&host);
            scope.spawn(move || host.build_named_type_export_route_entry(barrel, "P"))
        };
        spin_until("flight parked pre-fence", || {
            parked_pre_fence.load(Ordering::SeqCst)
        });
        land_unrelated_route_mutation(&host, other, leaf);
        release.store(true, Ordering::SeqCst);
        flight.join().unwrap()
    });
    *host.materialize_seam_hook.lock() = None;

    let (route_result, facts) = entry.expect("the fenced walk must still serve its caller");
    match &route_result {
        crate::resolver_core::RouteResult::Resolved {
            defining_canonical,
            defining_symbol,
        } => {
            assert_eq!(
                (defining_canonical.as_str(), defining_symbol.as_str()),
                (leaf, "P"),
                "the fenced walk resolves the route for its own caller",
            );
        }
        other => panic!("expected a resolved route, got {other:?}"),
    }
    assert!(
        facts.is_empty(),
        "a route entry whose walk consumed a fenced (ReturnOnly) shallow \
         serve must return EMPTY facts — RouteDb's strict admission treats \
         an empty fact signature as serve-without-admission (got {} facts)",
        facts.len(),
    );

    // No over-decline: an unfenced rebuild produces an admissible entry.
    let (route_result, facts) = host
        .build_named_type_export_route_entry(barrel, "P")
        .expect("the unfenced walk serves");
    assert!(
        matches!(
            &route_result,
            crate::resolver_core::RouteResult::Resolved { defining_canonical, defining_symbol }
                if defining_canonical == leaf && defining_symbol == "P"
        ),
        "the unfenced walk resolves the same route",
    );
    assert!(
        !facts.is_empty(),
        "an unfenced walk over store-published serves records its \
         participant facts and stays admissible",
    );
}

/// The lazy SFC analysis workers count their own structure parse on the
/// `sfc_parses` rail — counting lives INSIDE the worker (the
/// `parse::parse_sfc_counted` chokepoint), so no caller can run an
/// uncounted parse and a lost worker-internal increment fails here.
#[test]
fn lazy_sfc_analysis_workers_count_their_structure_parse() {
    use std::sync::atomic::Ordering;

    let provenance = crate::types::MetaProvenance::default();
    let source = "<script setup lang=\"ts\">const a: number = 1;</script>\n\
                  <template><div>{{ a }}</div></template>\n\
                  <style>.x { color: red; }</style>\n";

    let analysis = crate::parse::build_script_analysis_from_source(source, &provenance);
    assert_eq!(
        provenance.sfc_parses.load(Ordering::Relaxed),
        1,
        "build_script_analysis_from_source runs exactly ONE counted SFC \
         structure parse",
    );
    assert!(
        !analysis.bindings.is_empty(),
        "sanity: the worker really analysed the script (the parse was \
         consumed, not skipped)",
    );

    let styles = crate::parse::build_style_analyses_from_source(
        source,
        "/workspace/src/lazy.vue",
        &provenance,
    );
    assert_eq!(
        provenance.sfc_parses.load(Ordering::Relaxed),
        2,
        "build_style_analyses_from_source runs exactly ONE counted SFC \
         structure parse",
    );
    assert_eq!(
        styles.len(),
        1,
        "sanity: the worker really analysed the style block",
    );
}

/// Host-path pin for the counted SFC structure-parse rail: a `.vue`
/// upsert runs exactly one counted structure parse (the
/// `parse_vue_snapshot` lane), and the cold `IndexedReady` build reuses
/// the scheduler's cached parse — zero additional structure parses. A
/// host lane that runs an uncounted parse cannot be caught here, which
/// is why every `parse_sfc` execution routes through the counted
/// chokepoint (guard:
/// `sfc_structure_parse_routes_through_the_counted_chokepoint`); this
/// test pins that the counted lanes report the exact expected total.
#[test]
fn vue_upsert_and_cold_build_run_exactly_one_counted_sfc_parse() {
    let host = make_host(&[]);
    let canonical = "/workspace/src/counted.vue";
    host.provenance().reset();
    upsert(
        &host,
        canonical,
        "<script setup lang=\"ts\">defineProps<{ a: number }>()</script>\n\
         <template><div/></template>\n",
    );
    assert_eq!(
        snap(&host).sfc_parses,
        1,
        "a .vue upsert runs exactly ONE counted SFC structure parse",
    );

    let indexed = host
        .ensure_indexed_ready(canonical)
        .expect("the .vue canonical must materialise");
    assert!(
        indexed.framework_parse.is_some(),
        "sanity: the cold build carries the SFC parse",
    );
    assert_eq!(
        snap(&host).sfc_parses,
        1,
        "the cold IndexedReady build must reuse the scheduler's cached \
         SFC parse — zero additional structure parses",
    );
}

/// Overlay route discovery consults the view's overlay maps for HELPER
/// canonicals (`resolve_relative_overlay_candidate` probes
/// `view.content_hash_for` / `view.source` per candidate), so an
/// UNMASKED owner — one the view carries no overlay for — can still
/// bake an overlay-only helper route. Such a view-influenced artifact
/// must never publish under the owner's base (legacy) key: a base-host
/// read would observe session-overlay route state. The materialiser
/// serves the requesting session and declines the base-keyed publish.
#[test]
fn unmasked_owner_with_overlay_only_helper_route_never_publishes_base_key() {
    use crate::session_view::OverlaidView;

    let host = make_host(&[]);
    let owner = "/workspace/src/owner_unmasked.ts";
    upsert(
        &host,
        owner,
        "import type { H } from './helper';\nexport type Owner = H;\n",
    );

    // The helper exists ONLY as a session overlay — no disk presence,
    // so the base workspace cannot resolve `./helper`.
    let helper = "/workspace/src/helper.ts";
    let mut overlays: rustc_hash::FxHashMap<String, Arc<str>> = rustc_hash::FxHashMap::default();
    overlays.insert(helper.to_string(), Arc::from("export type H = { h: 1 };\n"));
    let view = OverlaidView::new(Arc::clone(&host), overlays);

    let served = host
        .materialize_overlay_indexed_ready_with_view(owner, &view)
        .expect("the materialiser must serve the requesting session");
    // Sanity: the served artifact really is view-influenced — route
    // discovery resolved the overlay-only helper for the unmasked owner.
    assert_eq!(
        served
            .import_routes
            .get("./helper")
            .and_then(|r| r.resolved_canonical_id.as_deref()),
        Some(helper),
        "sanity: overlay route discovery must resolve the overlay-only \
         helper for the unmasked owner (the view-influence this test \
         isolates)",
    );
    // The view-influenced artifact must NOT be readable through the
    // base (legacy) key space. The owner is unmasked, so the served
    // `whole_hash` IS the base content hash — exactly the key a base
    // content-pinned read would use.
    assert!(
        host.project_type_store()
            .indexed()
            .get(owner, served.whole_hash)
            .is_none(),
        "a view-influenced artifact for an unmasked owner must not be \
         published under the owner's base key — base reads would observe \
         session-overlay route state",
    );
}

/// Parameterised sibling of [`park_first_materialize_pre_fence`]: parks
/// the materialize seam at exactly the `n`-th firing (0-based). Each
/// IndexedReady flight fires the seam TWICE — once at flight start
/// (before any content read) and once pre-fence (after the artifact is
/// fully built against the pre-mutation state, before the publish
/// fence) — so to fence the k-th flight of a choreography pre-fence,
/// park `n = 2k + 1`. Later firings pass through unparked.
fn park_nth_materialize_pre_fence(
    host: &VerterHost,
    n: usize,
) -> (
    Arc<std::sync::atomic::AtomicBool>,
    Arc<std::sync::atomic::AtomicBool>,
) {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    let parked = Arc::new(AtomicBool::new(false));
    let release = Arc::new(AtomicBool::new(false));
    let seam_calls = Arc::new(AtomicUsize::new(0));
    {
        let parked = Arc::clone(&parked);
        let release = Arc::clone(&release);
        *host.materialize_seam_hook.lock() = Some(Arc::new(move || {
            if seam_calls.fetch_add(1, Ordering::SeqCst) == n {
                parked.store(true, Ordering::SeqCst);
                spin_until("seam release", || release.load(Ordering::SeqCst));
            }
        }));
    }
    (parked, release)
}

/// ReturnOnly never publishes — SEMANTIC-MEMO arm (the dispatch-tier
/// publication chain: a builder's `ctx.ensure_indexed_ready_serve`
/// consumes a FENCED serve → the build's value basis is a
/// served-without-publication artifact → the memo entry's fact stamps
/// (`dep_signature_for` reads the LIVE project generation; the traced
/// facts validate against a fresh view) cannot be rejected read-side.
/// The build must refuse memo admission (`cache_suppress`) while the
/// value still flows to the caller.
#[test]
fn fenced_indexed_serve_semantic_memo_build_is_served_but_not_admitted() {
    use std::sync::atomic::Ordering;

    use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    use crate::semantic_query::{
        QueryResult, ResolveDeclKey, ScopeId, SemanticQueryApi, SemanticQueryKey,
    };

    let host = make_host(&[]);
    let owner = "/workspace/src/owner.ts";
    let other = "/workspace/src/other.ts";
    upsert(&host, other, "export type Other = { o: 1 };\n");
    upsert(&host, owner, "export type Owner = { a: 1 };\n");
    host.provenance().reset();

    let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: ScopeId {
            canonical_id: Arc::from(owner),
            local_scope: None,
        },
        name: Arc::from("Owner"),
    });

    // Park the owner's IndexedReady flight pre-fence (flight 0 →
    // seam call 1), land a foreign-content mutation, release: the
    // builder's serve is FENCED (served, never published).
    let (parked_pre_fence, release) = park_nth_materialize_pre_fence(&host, 1);

    let result = std::thread::scope(|scope| {
        let flight = {
            let host = Arc::clone(&host);
            let key = key.clone();
            scope.spawn(move || {
                let dispatch = ProjectSemanticDispatch::new(&*host);
                dispatch.execute_type_node(key)
            })
        };
        spin_until("flight parked pre-fence", || {
            parked_pre_fence.load(Ordering::SeqCst)
        });
        upsert(&host, other, "export type Other = { o: 2 };\n");
        release.store(true, Ordering::SeqCst);
        flight.join().unwrap()
    });
    *host.materialize_seam_hook.lock() = None;

    assert!(
        matches!(result, QueryResult::Value(_)),
        "the fenced build must still serve its own caller a value \
         (got {result:?})",
    );
    // Choreography proof: the owner's flight really was fenced — the
    // artifact store holds NO published IndexedReady for the owner's
    // current content.
    let current_hash = host
        .get_whole_hash(owner)
        .expect("owner tracked by the scheduler");
    assert!(
        host.project_type_store()
            .indexed()
            .get(owner, current_hash)
            .is_none(),
        "choreography: the parked flight must have been fenced (nothing \
         published for the owner's current content)",
    );

    // The discriminator: a memo entry whose value basis was a fenced
    // (ReturnOnly) IndexedReady serve must NOT be published warm.
    assert!(
        host.project_type_store()
            .semantic_graph()
            .get_unvalidated(&key)
            .is_none(),
        "a semantic-memo entry built from a fenced (ReturnOnly) \
         IndexedReady serve must not be admitted warm — its fact stamps \
         are read from the LIVE post-mutation state while its value was \
         computed FROM the superseded artifact",
    );

    // No over-decline: an unfenced re-execute publishes warm.
    let result = {
        let dispatch = ProjectSemanticDispatch::new(&*host);
        dispatch.execute_type_node(key.clone())
    };
    assert!(
        matches!(result, QueryResult::Value(_)),
        "the unfenced re-execute serves a value",
    );
    assert!(
        host.project_type_store()
            .semantic_graph()
            .get_unvalidated(&key)
            .is_some(),
        "a build from store-published serves is admitted warm",
    );
}

/// ReturnOnly never publishes — CLASS-SURFACE STATIC arm over the
/// export-target fallback rail. A `ResolveClassSurface(Static)` key whose
/// slot names a RE-EXPORTING barrel rebases onto the declaring identity
/// and self-roots on BOTH files' content versions. When the declaring
/// file's `IndexedReady` flight is FENCED mid-build (a foreign mutation
/// lands between artifact build and publish fence), the composed surface's
/// value basis is a served-without-publication artifact: the build must
/// refuse memo admission (the chokepoint flag reaches the dispatch
/// executor's `cache_suppress` gate) while the value still flows to the
/// caller — and an unfenced re-execute must publish warm (no over-decline).
#[test]
fn fenced_declaring_serve_class_surface_static_is_served_but_not_admitted() {
    use std::sync::atomic::Ordering;

    use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    use crate::semantic_query::{
        ClassSurfaceContext, ClassSurfaceSide, QueryResult, ResolvedDeclSlotIdentity,
        SemanticQueryApi, SemanticQueryKey,
    };

    let host = make_host(&[]);
    let barrel = "/workspace/src/barrel.ts";
    let origin = "/workspace/src/origin.ts";
    let other = "/workspace/src/other.ts";
    upsert(&host, other, "export type Other = { o: 1 };\n");
    upsert(
        &host,
        origin,
        "export class Klass { static own(): number { return 0; } }\n",
    );
    upsert(&host, barrel, "export { Klass } from './origin';\n");

    let env = host.host_view_env_hashes_for(barrel);
    let project_identity = host.host_view_project_identity_for(barrel).fold_u32();
    let key = SemanticQueryKey::ResolveClassSurface {
        decl_slot: ResolvedDeclSlotIdentity::type_slot(
            Arc::from(barrel),
            Arc::from("Klass"),
            project_identity,
            env.type_env_hash,
            env.lib_env_hash,
        ),
        type_args: Arc::from(Vec::new().into_boxed_slice()),
        side: ClassSurfaceSide::Static,
        context: ClassSurfaceContext {
            parse_env_hash: env.parse_env_hash,
            resolve_env_hash: env.resolve_env_hash,
            mode: ProjectionMode::Shallow,
        },
    };

    // Park the DECLARING file's IndexedReady flight pre-fence: the
    // barrel-keyed build materialises the barrel first (flight 0), then
    // the export-target walk reaches the origin (flight 1 → seam call
    // 2·1 + 1 = 3). Land a foreign-content mutation while parked,
    // release: the origin's serve is FENCED (served, never published).
    let (parked_pre_fence, release) = park_nth_materialize_pre_fence(&host, 3);

    let result = std::thread::scope(|scope| {
        let flight = {
            let host = Arc::clone(&host);
            let key = key.clone();
            scope.spawn(move || {
                let dispatch = ProjectSemanticDispatch::new(&*host);
                dispatch.execute_type_node(key)
            })
        };
        spin_until("declaring flight parked pre-fence", || {
            parked_pre_fence.load(Ordering::SeqCst)
        });
        upsert(&host, other, "export type Other = { o: 2 };\n");
        release.store(true, Ordering::SeqCst);
        flight.join().unwrap()
    });
    *host.materialize_seam_hook.lock() = None;

    assert!(
        matches!(result, QueryResult::Value(_)),
        "the fenced build must still serve its own caller a value \
         (got {result:?})",
    );
    // The discriminator: a class-surface memo entry whose declaring-file
    // value basis was a fenced (ReturnOnly) serve must NOT be published
    // warm — its self-roots / fact stamps read the LIVE post-mutation
    // state while the composed surface was computed FROM the superseded
    // artifact. (Vacuity guard built in: if the park fenced nothing, the
    // build publishes and this assertion fails.)
    assert!(
        host.project_type_store()
            .semantic_graph()
            .get_unvalidated(&key)
            .is_none(),
        "a class-surface memo entry built from a fenced (ReturnOnly) \
         declaring-file serve must not be admitted warm",
    );

    // No over-decline: an unfenced re-execute publishes warm.
    let result = {
        let dispatch = ProjectSemanticDispatch::new(&*host);
        dispatch.execute_type_node(key.clone())
    };
    assert!(
        matches!(result, QueryResult::Value(_)),
        "the unfenced re-execute serves a value",
    );
    assert!(
        host.project_type_store()
            .semantic_graph()
            .get_unvalidated(&key)
            .is_some(),
        "a class-surface build from store-published serves is admitted warm",
    );
}

/// ReturnOnly never publishes — OWNER-IMPORT-SURFACE arm. The
/// per-binding route walk inside the surface producer signals a fenced
/// (ReturnOnly) underlying serve by returning the resolved root with
/// EMPTY route facts (`RouteDb` / `ImportedRootDb` honour that and
/// never persist); the surface producer must honour the same signal:
/// serve the surface to its caller WITHOUT admitting it into
/// `OwnerImportSurfaceDb` — the persisted entry would bind the
/// fenced-resolved target with direct-hop facts that validate live.
#[test]
fn owner_import_surface_from_fenced_route_walk_is_served_but_not_admitted() {
    use std::sync::atomic::Ordering;

    let host = make_host(&[]);
    let owner = "/workspace/src/owner.ts";
    let barrel = "/workspace/src/index.ts";
    let leaf = "/workspace/src/p.ts";
    let other = "/workspace/src/other.ts";
    upsert(&host, leaf, "export type P = { a: 1 };\n");
    upsert(&host, other, "export type Other = { o: 1 };\n");
    upsert(&host, barrel, "export type { P } from './p';\n");
    upsert(
        &host,
        owner,
        "import type { P } from './index';\nexport type Owner = P;\n",
    );
    host.provenance().reset();

    // Choreography: flight 0 = owner's own IndexedReady (the surface
    // producer reads the owner shallow state first); flight 1 = the
    // barrel's IndexedReady (the walk's provider read). Park the
    // BARREL's flight pre-fence (seam call 3 = 2*1 + 1).
    let (parked_pre_fence, release) = park_nth_materialize_pre_fence(&host, 3);

    let resolved = std::thread::scope(|scope| {
        let flight = {
            let host = Arc::clone(&host);
            scope.spawn(move || host.resolve_owner_direct_import(owner, "P"))
        };
        spin_until("flight parked pre-fence", || {
            parked_pre_fence.load(Ordering::SeqCst)
        });
        upsert(&host, other, "export type Other = { o: 2 };\n");
        release.store(true, Ordering::SeqCst);
        flight.join().unwrap()
    });
    *host.materialize_seam_hook.lock() = None;

    // Choreography proof: the barrel's flight really was fenced.
    let barrel_hash = host
        .get_whole_hash(barrel)
        .expect("barrel tracked by the scheduler");
    assert!(
        host.project_type_store()
            .indexed()
            .get(barrel, barrel_hash)
            .is_none(),
        "choreography: the parked barrel flight must have been fenced \
         (nothing published for the barrel's current content)",
    );
    let (final_canonical, final_name) =
        resolved.expect("the fenced walk must still serve its caller the resolved root");
    assert_eq!(
        (final_canonical.as_str(), final_name.as_str()),
        (leaf, "P"),
        "the fenced walk resolves the direct import for its own caller",
    );

    // The discriminator: the surface built over the fenced walk must
    // NOT have been admitted into OwnerImportSurfaceDb.
    let owner_hash = host
        .get_whole_hash(owner)
        .expect("owner tracked by the scheduler");
    assert!(
        host.project_type_store()
            .owner_import_surfaces()
            .get(owner, owner_hash)
            .is_none(),
        "an owner-import surface whose per-binding route walk consumed a \
         FENCED (ReturnOnly) serve must not be admitted — the persisted \
         entry would bind the fenced-resolved target with facts that \
         validate against the live view",
    );

    // No over-decline: an unfenced rebuild admits the surface.
    let resolved = host
        .resolve_owner_direct_import(owner, "P")
        .expect("the unfenced rebuild serves");
    assert_eq!(
        (resolved.0.as_str(), resolved.1.as_str()),
        (leaf, "P"),
        "the unfenced rebuild resolves the same root",
    );
    assert!(
        host.project_type_store()
            .owner_import_surfaces()
            .get(owner, owner_hash)
            .is_some(),
        "a surface built over store-published serves is admitted",
    );
}

/// ReturnOnly never publishes — BUNDLE-SINGLEFLIGHT rendezvous arm. A
/// fenced-derived prepared-decl bundle must NOT be retained as a
/// joinable rendezvous on the bundle singleflight: a claimant that
/// arrives after the fenced leader completed must re-run the cold
/// build against fresh state instead of adopting the fenced-derived
/// bundle with no ReturnOnly signal (its enclosing compute could warm
/// shared caches with it).
#[test]
fn fenced_bundle_flight_is_not_a_joinable_rendezvous() {
    use std::sync::atomic::Ordering;

    use crate::resolver_core::StoreView;

    let host = make_host(&[]);
    let owner = "/workspace/src/owner.ts";
    let dep = "/workspace/src/dep.ts";
    let other = "/workspace/src/other.ts";
    upsert(&host, dep, "export type P = { a: 1 };\n");
    upsert(&host, other, "export type Other = { o: 1 };\n");
    upsert(
        &host,
        owner,
        "import type { P } from './dep';\nexport type Owner = P;\n",
    );
    host.provenance().reset();

    // One PRE-mutation view shared by the leader and the late claimant
    // so both land on the SAME singleflight lane (the lane key folds
    // the view compat token).
    let view = host
        .resolver_store_view_read()
        .into_cold_seed_view()
        .into_inner();

    // Keep the leader's bundle lane alive past the leader's completion
    // (a participation pin), so the late claimant observes exactly what
    // the leader's flight left behind: a retained Done rendezvous
    // (pre-fix) or a non-joinable lane (post-fix).
    let lane_pin = host
        .resolver
        .runtime
        .prepared_decl_bundles
        .singleflight()
        .participate(owner.to_string(), view.compat_token());

    // Park the owner's IndexedReady flight (inside the leader's bundle
    // cold build) pre-fence: flight 0 → seam call 1.
    let (parked_pre_fence, release) = park_nth_materialize_pre_fence(&host, 1);

    let bundle = std::thread::scope(|scope| {
        let flight = {
            let host = Arc::clone(&host);
            let view = &view;
            scope.spawn(move || host.prepared_decl_bundle_with_store_view(view, owner))
        };
        spin_until("flight parked pre-fence", || {
            parked_pre_fence.load(Ordering::SeqCst)
        });
        upsert(&host, other, "export type Other = { o: 2 };\n");
        release.store(true, Ordering::SeqCst);
        flight.join().unwrap()
    });
    assert!(
        bundle.is_some(),
        "the fenced leader must still serve its own caller a bundle",
    );
    assert_eq!(
        snap(&host).bundle_materializations,
        1,
        "sanity: the leader cold-built the bundle",
    );

    // The discriminator: a late claimant on the SAME lane must NOT
    // adopt the fenced-derived bundle as a joinable rendezvous — it
    // re-runs the cold build against fresh state.
    host.provenance().reset();
    let second = host.prepared_decl_bundle_with_store_view(&view, owner);
    assert!(second.is_some(), "the late claimant must be served");
    assert_eq!(
        snap(&host).bundle_materializations,
        1,
        "a fenced-derived bundle must not be retained as a joinable \
         rendezvous — the late claimant re-runs the cold build against \
         fresh state instead of adopting it (0 materialisations means \
         the fenced bundle was adopted with no ReturnOnly signal)",
    );
    drop(lane_pin);
    *host.materialize_seam_hook.lock() = None;

    // No over-decline: the late claimant's rebuild ran unfenced and was
    // admitted — a fresh read serves it warm.
    host.provenance().reset();
    let third = host.prepared_decl_bundle(owner);
    assert!(third.is_some(), "the warm read must serve");
    let provenance = snap(&host);
    assert_eq!(
        provenance.bundle_cache_hits, 1,
        "the unfenced rebuild was admitted and serves warm",
    );
    assert_eq!(
        provenance.bundle_materializations, 0,
        "the warm read must not rebuild",
    );
}

/// ReturnOnly never publishes — BUNDLE-SINGLEFLIGHT fenced-MISS arm
/// (the `None` sibling of
/// `fenced_bundle_flight_is_not_a_joinable_rendezvous`). The bundle
/// producers conclude "no bundle" from the SERVED artifact's
/// surface-emptiness — and the serve can be FENCED (ReturnOnly,
/// `store_published == false`): the emptiness then describes a
/// superseded artifact, not live content, so the miss is NOT
/// reproducible and must not be retained as a joinable rendezvous. A
/// burst member that adopted it would treat a canonical that HAS a
/// live declaration surface as bundle-less with no ReturnOnly signal
/// on its own request.
#[test]
fn fenced_surface_empty_miss_is_not_a_joinable_rendezvous() {
    use std::sync::atomic::Ordering;

    use crate::resolver_core::StoreView;

    let host = make_host(&[]);
    let owner = "/workspace/src/surface_empty_owner.ts";
    let other = "/workspace/src/other.ts";
    upsert(&host, other, "export type Other = { o: 1 };\n");
    // The owner's pre-mutation content has NO declaration surface — the
    // shape from which both bundle producers conclude a miss.
    upsert(&host, owner, "// no declaration surface yet\n");
    host.provenance().reset();

    // One PRE-mutation view shared by the leader and the late claimant
    // so both land on the SAME singleflight lane (the lane key folds
    // the view compat token).
    let view = host
        .resolver_store_view_read()
        .into_cold_seed_view()
        .into_inner();

    // Keep the leader's bundle lane alive past the leader's completion
    // (a participation pin), so the late claimant observes exactly what
    // the leader's flight left behind: a retained Done(None) rendezvous
    // (pre-fix) or a non-joinable lane (post-fix).
    let lane_pin = host
        .resolver
        .runtime
        .prepared_decl_bundles
        .singleflight()
        .participate(owner.to_string(), view.compat_token());

    // Park the owner's IndexedReady flight (inside the leader's bundle
    // cold build) pre-fence: flight 0 → seam call 1.
    let (parked_pre_fence, release) = park_nth_materialize_pre_fence(&host, 1);

    let miss = std::thread::scope(|scope| {
        let flight = {
            let host = Arc::clone(&host);
            let view = &view;
            scope.spawn(move || host.prepared_decl_bundle_with_store_view(view, owner))
        };
        spin_until("flight parked pre-fence", || {
            parked_pre_fence.load(Ordering::SeqCst)
        });
        upsert(&host, other, "export type Other = { o: 2 };\n");
        release.store(true, Ordering::SeqCst);
        flight.join().unwrap()
    });
    *host.materialize_seam_hook.lock() = None;
    assert!(
        miss.is_none(),
        "the fenced leader's artifact is surface-empty — its own caller \
         is served the miss (its request pre-dates the mutation)",
    );

    // Live content moves on: the owner NOW has a declaration surface.
    upsert(&host, owner, "export type Foo = { x: 1 };\n");

    // The discriminator: a late claimant on the SAME lane must NOT
    // adopt the fenced-derived miss as a joinable rendezvous — it
    // re-runs the cold build against live state and finds the surface.
    let second = host.prepared_decl_bundle_with_store_view(&view, owner);
    drop(lane_pin);
    let bundle = second.expect(
        "a fenced-derived surface-empty miss must not be retained as a \
         joinable rendezvous — the late claimant re-runs the cold build \
         against live state (None means the non-reproducible miss was \
         adopted with no ReturnOnly signal)",
    );
    assert!(
        bundle.prepared_type_decls.contains_key("Foo"),
        "the late claimant's rebuild reflects the live declaration surface",
    );

    // No over-decline: the claimant's unfenced rebuild was admitted —
    // a fresh read serves it warm.
    host.provenance().reset();
    let third = host.prepared_decl_bundle(owner);
    assert!(third.is_some(), "the warm read must serve");
    let provenance = snap(&host);
    assert_eq!(
        provenance.bundle_cache_hits, 1,
        "the unfenced rebuild was admitted and serves warm",
    );
    assert_eq!(
        provenance.bundle_materializations, 0,
        "the warm read must not rebuild",
    );
}

/// Negative control for the fenced-MISS arm: a surface-empty miss
/// derived from a STORE-PUBLISHED (unfenced) serve IS a reproducible
/// miss and stays a joinable rendezvous — a burst member on the same
/// lane adopts it without re-running the cold build. Proves the
/// stability decision consults the serve's publication status rather
/// than declining every miss.
#[test]
fn unfenced_surface_empty_miss_stays_a_joinable_rendezvous() {
    use crate::resolver_core::StoreView;

    let host = make_host(&[]);
    let owner = "/workspace/src/surface_empty_owner.ts";
    upsert(&host, owner, "// no declaration surface\n");
    host.provenance().reset();

    let view = host
        .resolver_store_view_read()
        .into_cold_seed_view()
        .into_inner();
    let lane_pin = host
        .resolver
        .runtime
        .prepared_decl_bundles
        .singleflight()
        .participate(owner.to_string(), view.compat_token());

    let miss = host.prepared_decl_bundle_with_store_view(&view, owner);
    assert!(miss.is_none(), "the surface-empty owner has no bundle");
    assert_eq!(
        snap(&host).bundle_cold_flight_runs,
        1,
        "sanity: the leader ran the cold flight body once",
    );
    assert_eq!(
        snap(&host).indexed_ready_materializes,
        1,
        "sanity: the leader cold-materialised the owner's IndexedReady",
    );

    // A burst member on the SAME lane adopts the reproducible miss
    // without re-running the cold build. The discriminator is the
    // FLIGHT-BODY run counter, not a materialisation counter: an
    // over-declined miss would make the burst member re-run the cold
    // build, but that re-run warm-hits the leader's published
    // IndexedReady and re-concludes surface-emptiness without bumping
    // any materialisation counter — only the flight-body run itself
    // separates adopt from re-run.
    host.provenance().reset();
    let second = host.prepared_decl_bundle_with_store_view(&view, owner);
    drop(lane_pin);
    assert!(
        second.is_none(),
        "the unfenced surface-empty miss is reproducible — the burst \
         member is served the same miss",
    );
    assert_eq!(
        snap(&host).bundle_cold_flight_runs,
        0,
        "the unfenced miss stays a joinable rendezvous — the burst \
         member must NOT re-run the cold flight body (1 means every \
         miss is being declined retention)",
    );
}

/// ReturnOnly never publishes — TEMPLATE-ANALYSIS persist arm. The
/// `get_raw_analysis_snapshot` FileArtifactStore lane threads the served
/// artifact's own `raw_source`/`cached_parse` into the lazy
/// template-analysis computation, which persists the derived template
/// into the shared base `derived_raw_cache`. The serve can be FENCED
/// (ReturnOnly, `store_published == false`): the artifact then describes
/// superseded state, and the persisted entry — which carries NO
/// content rail and is served as current by every subsequent template
/// read — would outlive the upsert-time clear. The publication status
/// must flow BY VALUE with the threaded inputs and DECLINE the persist,
/// while the fenced caller is still served the full snapshot (template
/// included).
#[test]
fn fenced_indexed_serve_template_analysis_is_served_but_not_persisted() {
    use std::sync::atomic::Ordering;

    // VFS-only `.vue` canonical (no upsert): the scheduler misses at
    // the raw-snapshot entry, so the read routes through the
    // FileArtifactStore lane and joins the IndexedReady flight.
    let host = make_host(&[(VUE_OWNER, VUE_FIXTURE)]);
    let other = "/workspace/src/other.ts";
    upsert(&host, other, "export type Other = { o: 1 };\n");
    host.provenance().reset();

    // Park the owner's IndexedReady flight pre-fence: flight 0 → seam
    // call 1.
    let (parked_pre_fence, release) = park_nth_materialize_pre_fence(&host, 1);

    let snapshot = std::thread::scope(|scope| {
        let flight = {
            let host = Arc::clone(&host);
            scope.spawn(move || host.get_raw_analysis_snapshot(VUE_OWNER))
        };
        spin_until("flight parked pre-fence", || {
            parked_pre_fence.load(Ordering::SeqCst)
        });
        upsert(&host, other, "export type Other = { o: 2 };\n");
        release.store(true, Ordering::SeqCst);
        flight.join().unwrap()
    });
    *host.materialize_seam_hook.lock() = None;

    // ReturnOnly still serves its own caller: the fenced request
    // pre-dates the mutation, so the snapshot — template included — is
    // valid for THIS read.
    let snapshot = snapshot.expect("the fenced serve still answers its own caller");
    assert!(
        snapshot.template.is_some(),
        "the fenced serve must still carry template analysis for its caller",
    );

    // The discriminator: the template derived from the FENCED serve's
    // artifact must NOT have been persisted into the shared base
    // `derived_raw_cache` — the entry has no content rail, so every
    // subsequent template read would serve the superseded-bytes value
    // as current.
    assert!(
        host.derived_raw_cache()
            .get(VUE_OWNER)
            .and_then(|cc| {
                cc.raw_template_analysis()
                    .map(|entry| Arc::clone(&entry.template))
            })
            .is_none(),
        "a template derived from a FENCED (ReturnOnly) IndexedReady serve \
         must not be persisted into the shared derived_raw_cache",
    );

    // No over-decline: the flight's `ensure_loaded` ingressed the
    // canonical, so the next read serves from the LIVE scheduler entry
    // and ITS persist (store-authoritative inputs) must land.
    let second = host
        .get_raw_analysis_snapshot(VUE_OWNER)
        .expect("the scheduler-backed read must serve");
    assert!(
        second.template.is_some(),
        "the scheduler-backed read must carry template analysis",
    );
    assert!(
        host.derived_raw_cache()
            .get(VUE_OWNER)
            .and_then(|cc| {
                cc.raw_template_analysis()
                    .map(|entry| Arc::clone(&entry.template))
            })
            .is_some(),
        "a template computed from store-authoritative inputs is persisted \
         — the fenced decline must not suppress the live lane",
    );
}

/// ReturnOnly never publishes — AUGMENTATION-PROBE fail-closed arm.
/// `owner_has_module_augmentation_dependency` materialises the owner's
/// dependency closure and reads each dependency's published artifacts
/// row for augmentation facts. A successful-but-FENCED serve published
/// nothing, so the augmenter silently contributes no facts and the
/// probe fails OPEN: a `Content`-mode compile admits a
/// content-addressed entry carrying no augmenter fingerprint into the
/// one cache family with NO read-side fact rail. The probe must fail
/// CLOSED: any fenced serve in the walk means the augmentation
/// inventory is unverifiable for this request → report `true` (floors
/// the compile to the fact-validated route).
#[test]
fn augmentation_probe_fails_closed_on_fenced_serve() {
    use std::sync::atomic::Ordering;

    let host = make_host(&[]);
    let owner = "/workspace/src/owner.ts";
    let aug = "/workspace/src/aug.ts";
    let other = "/workspace/src/other.ts";
    upsert(&host, other, "export type Other = { o: 1 };\n");
    upsert(
        &host,
        aug,
        "declare global { interface VerterProbeGlobal { a: 1 } }\nexport {};\n",
    );
    upsert(
        &host,
        owner,
        "import './aug';\nexport type Owner = { a: 1 };\n",
    );
    host.provenance().reset();

    // Choreography: flight 0 = the owner's IndexedReady; flight 1 = the
    // augmenter's IndexedReady (the side-effect-import BFS walk). Park
    // the AUGMENTER's flight pre-fence (seam call 3 = 2*1 + 1).
    let (parked_pre_fence, release) = park_nth_materialize_pre_fence(&host, 3);

    let has_augmentation = std::thread::scope(|scope| {
        let flight = {
            let host = Arc::clone(&host);
            scope.spawn(move || host.owner_has_module_augmentation_dependency(owner))
        };
        spin_until("flight parked pre-fence", || {
            parked_pre_fence.load(Ordering::SeqCst)
        });
        upsert(&host, other, "export type Other = { o: 2 };\n");
        release.store(true, Ordering::SeqCst);
        flight.join().unwrap()
    });
    *host.materialize_seam_hook.lock() = None;

    // Choreography proof: the augmenter's flight really was fenced.
    let aug_hash = host
        .get_whole_hash(aug)
        .expect("augmenter tracked by the scheduler");
    assert!(
        host.project_type_store()
            .indexed()
            .get(aug, aug_hash)
            .is_none(),
        "choreography: the parked augmenter flight must have been fenced \
         (nothing published for the augmenter's current content)",
    );

    // The discriminator: with the augmenter's artifacts row unreadable
    // after a successful-but-fenced serve, the probe must fail CLOSED.
    assert!(
        has_augmentation,
        "the augmentation probe must fail CLOSED on a fenced (ReturnOnly) \
         serve — a fenced augmenter published no artifacts row, so its \
         augmentation facts are unverifiable and Content-mode admission \
         (no read-side fact rail) must be refused",
    );

    // No over-close: the unfenced probe still reports the real
    // augmentation dependency...
    assert!(
        host.owner_has_module_augmentation_dependency(owner),
        "the unfenced probe reports the genuine global augmenter",
    );
    // ...and an owner with NO augmenting dependency closure still
    // probes false (the fail-closed arm fires only on fenced serves).
    // Fresh host: the global augmenter above reaches EVERY owner on
    // its host, so the negative control needs an augmenter-free world.
    let plain_host = make_host(&[]);
    let plain_owner = "/workspace/src/plain_owner.ts";
    let plain_dep = "/workspace/src/plain_dep.ts";
    upsert(&plain_host, plain_dep, "export type D = { d: 1 };\n");
    upsert(
        &plain_host,
        plain_owner,
        "import type { D } from './plain_dep';\nexport type PlainOwner = D;\n",
    );
    assert!(
        !plain_host.owner_has_module_augmentation_dependency(plain_owner),
        "an owner with no augmenting closure still probes false after \
         the fail-closed arm lands",
    );
}

/// ReturnOnly never publishes — BAKED-EDGE consumption arm. A FENCED
/// (ReturnOnly) IndexedReady serve carries cross-file edges
/// (`import_targets[..].canonical_id`) baked against the PRE-mutation
/// file set; `cache_dependency_candidates_from_snapshot` must not
/// trust them — it re-resolves the raw source specifiers through the
/// live resolver, so the tracked-dependency set names the
/// post-mutation targets.
#[test]
fn fenced_serve_baked_edges_reresolve_in_dependency_candidates() {
    use std::sync::atomic::Ordering;

    let host = make_host(&[]);
    let owner = "/workspace/src/owner.ts";
    let dep = "/workspace/src/dep.ts";
    let dep2 = "/workspace/src/dep2.ts";
    upsert(&host, dep, "export type P = { a: 1 };\n");
    upsert(&host, dep2, "export type P = { a: 2 };\n");
    upsert(
        &host,
        owner,
        "import type { P } from './dep';\nexport type Owner = P;\n",
    );
    host.provenance().reset();

    // Park the owner's IndexedReady flight pre-fence (flight 0 → seam
    // call 1): the import edge `./dep → /workspace/src/dep.ts` is baked
    // against the pre-mutation route table. The mutation lands an
    // authoritative exact-resolution RETARGET of the owner's own
    // `./dep` specifier (bumps `project_generation`, fencing the
    // flight), so the baked edge and the live resolution genuinely
    // diverge.
    let (parked_pre_fence, release) = park_nth_materialize_pre_fence(&host, 1);

    let candidates = std::thread::scope(|scope| {
        let flight = {
            let host = Arc::clone(&host);
            scope.spawn(move || {
                let snapshot = crate::types::FileAnalysisSnapshot::default();
                host.cache_dependency_candidates_from_snapshot(owner, &snapshot)
            })
        };
        spin_until("flight parked pre-fence", || {
            parked_pre_fence.load(Ordering::SeqCst)
        });
        host.set_exact_resolutions(
            owner,
            vec![verter_workspace::ExactResolution {
                specifier: "./dep".to_string(),
                phase: verter_workspace::ResolvePhase::CodegenBlocker,
                kind: verter_workspace::ResolveRequestKind::TypeImport,
                resolved_canonical_id: Some(dep2.to_string()),
                possible_canonical_ids: vec![dep2.to_string()],
            }],
        );
        release.store(true, Ordering::SeqCst);
        flight.join().unwrap()
    });
    *host.materialize_seam_hook.lock() = None;

    // Choreography proof: the owner's flight really was fenced.
    let owner_hash = host
        .get_whole_hash(owner)
        .expect("owner tracked by the scheduler");
    assert!(
        host.project_type_store()
            .indexed()
            .get(owner, owner_hash)
            .is_none(),
        "choreography: the parked owner flight must have been fenced \
         (nothing published for the owner's current content)",
    );

    // The discriminator: the candidates derived from the fenced serve
    // must name the LIVE target (the exact-resolution retarget), not
    // the superseded baked edge.
    assert!(
        candidates.contains(dep2),
        "dependency candidates from a fenced (ReturnOnly) serve must \
         re-resolve the import edge against the live route authority — \
         the retargeted canonical is the live target (got {candidates:?})",
    );
    assert!(
        !candidates.contains(dep),
        "the superseded baked edge target must not enter the candidate \
         set when the serve was fenced (got {candidates:?})",
    );
}

/// Single-env internal consistency at the OVERLAY materialiser: the
/// overlay cold flight derives the eval-parse `SourceType` from the
/// OVERLAY content — the same content (and SFC structure parse) the
/// snapshot is built from — never from the scheduler's stamp, which
/// covers BASE content. An overlay flipping the script lang
/// (`lang="js"` base → `lang="ts"` overlay) must parse the overlay eval
/// source as TS: under the stale base type the TS body misparses
/// (fatal parse → empty env) and the overlay surface silently loses its
/// declarations while the snapshot still reports the overlay lang — an
/// intra-artifact divergence on the single-env artifact.
#[test]
fn overlay_lang_flip_parses_under_overlay_source_type() {
    use crate::session_view::OverlaidView;

    let host = make_host(&[]);
    let canonical = "/workspace/src/flip.vue";
    // Base: a JS script — the scheduler stamps the JS `SourceType`
    // from THIS content at upsert time.
    upsert(
        &host,
        canonical,
        "<script lang=\"js\">\nconst base = 1;\n</script>\n<template><div /></template>\n",
    );

    let mut overlays: rustc_hash::FxHashMap<String, Arc<str>> = rustc_hash::FxHashMap::default();
    overlays.insert(
        canonical.to_string(),
        Arc::from(
            "<script lang=\"ts\">\nexport interface Flip { a: string }\nexport const flip: Flip = { a: \"b\" };\n</script>\n<template><div /></template>\n",
        ),
    );
    let view = OverlaidView::new(Arc::clone(&host), overlays);

    let artifact = host
        .materialize_overlay_indexed_ready_with_view(canonical, &view)
        .expect("overlay artifact must materialise");
    assert!(
        artifact.shallow_state.symbol("Flip").is_some(),
        "the overlay eval env must carry the TS surface declared by the \
         overlay's `lang=\"ts\"` script — the eval parse must run under \
         the OVERLAY-derived source type, not the base scheduler stamp"
    );

    // Negative: the base artifact keeps the BASE surface — the overlay
    // lang flip never leaks into the base candidate.
    let base = host
        .ensure_indexed_ready(canonical)
        .expect("base artifact must materialise");
    assert!(
        base.shallow_state.symbol("Flip").is_none(),
        "the overlay TS surface must not leak into the base artifact"
    );
}

/// The scheduler Source stage is a VISIBLE lane on the
/// `non_sfc_snapshot_parses` rail: exactly ONE counted full-program
/// snapshot parse per non-SFC upsert. Discriminates the counter wiring
/// — if the worker stopped counting, this pin fails.
#[test]
fn scheduler_source_lane_counts_one_non_sfc_snapshot_parse() {
    let host = make_host(&[]);
    host.provenance().reset();
    upsert(&host, SCRATCH_ID, SCRATCH);
    let provenance = snap(&host);
    assert_eq!(
        provenance.non_sfc_snapshot_parses, 1,
        "one non-SFC upsert = exactly one counted scheduler snapshot \
         parse (got {})",
        provenance.non_sfc_snapshot_parses,
    );
}

/// A fatal (panicked) eval-program parse performs NO second
/// full-program parse: the flight's single eval-program parse is the
/// only parse attempt, and the fatal arm publishes the empty analysis
/// snapshot directly. A re-parse would run over the same bytes under
/// the same source type (`non_sfc_source_type` on both lanes for a
/// non-SFC canonical) and is guaranteed to panic identically, yielding
/// the same empty snapshot — so the `non_sfc_snapshot_parses` rail
/// stays at ZERO on the fatal lane.
#[test]
fn overlay_fatal_parse_publishes_empty_snapshot_without_second_parse() {
    use crate::session_view::OverlaidView;

    let host = make_host(&[]);
    let canonical = "/workspace/src/fatal.ts";
    upsert(&host, canonical, "export const ok = 1;\n");

    let mut overlays: rustc_hash::FxHashMap<String, Arc<str>> = rustc_hash::FxHashMap::default();
    // Unterminated string literal — an unrecoverable (panicked) OXC
    // parse, so `parse_eval_program` yields no program.
    overlays.insert(
        canonical.to_string(),
        Arc::from("export const broken = \"unterminated"),
    );
    let view = OverlaidView::new(Arc::clone(&host), overlays);

    host.provenance().reset();
    let artifact = host.materialize_overlay_indexed_ready_with_view(canonical, &view);
    let provenance = snap(&host);
    assert_eq!(
        provenance.eval_program_parses, 1,
        "the fatal overlay flight still performs exactly one eval-program \
         parse attempt (got {})",
        provenance.eval_program_parses,
    );
    assert_eq!(
        provenance.non_sfc_snapshot_parses, 0,
        "the fatal arm must not re-parse: same bytes + same source type \
         is a guaranteed identical panic, so the empty snapshot is \
         constructed directly with zero counted non-SFC snapshot parses \
         (got {})",
        provenance.non_sfc_snapshot_parses,
    );
    // The fatal flight still publishes an (empty-surface) artifact —
    // dropping the re-parse must not turn the fatal lane into a None.
    let artifact = artifact.expect("fatal overlay flight still materialises an artifact");
    assert!(
        artifact.shallow_state.symbol("broken").is_none(),
        "a fatally unparseable overlay carries no shallow symbols"
    );
}

// ── `.vue` cold-build script-parse dedup ─────────────────────────────
//
// The `.vue` snapshot build extracts the position-preserving script
// source from the SFC once and must OXC-parse it exactly ONCE: export
// signatures and script analysis walk the SAME program (the
// `_from_program` threading `build_non_sfc_snapshot_from_program`
// already uses for non-SFC files). A count of 2 means a consumer
// re-introduced a per-consumer re-parse of the same script bytes —
// the hidden-uncounted-parse class that made the parse-once cold-
// materialise claim false for `.vue` files. Counted on the
// `vue_script_snapshot_parses` provenance rail (inside the worker fn,
// so every lane counts).

const VUE_FIXTURE: &str = "<script setup lang=\"ts\">import { ref } from 'vue';\n\
     const n = ref(1);\n\
     export type P = { a: 1 };\n\
     </script>\n\
     <template><div>{{ n }}</div></template>\n";

const VUE_OWNER: &str = "/workspace/src/Comp.vue";

/// Base scheduler-miss lane: the canonical is known only to the VFS, so
/// the cold `ensure_indexed_ready` flight loads it through
/// `ensure_loaded` — the scheduler worker performs THE single ingress
/// parse pair (one SFC structure parse + one script-program parse,
/// both worker-counted) — and the flight then REUSES the committed
/// scheduler snapshot (`indexed_ready_scheduler_snapshot_reuse == 1`),
/// adding only its eval-program parse. The snapshot lane itself
/// re-parses nothing: the script-program rail stays at the worker's 1.
///
/// The worker's parse and the flight's eval-program parse cannot
/// collapse further: the worker's OXC arena is per-file/per-version
/// and dropped after lowering (never retained in host caches), so the
/// flight's borrowed eval program must be its own parse.
#[test]
fn vue_cold_build_parses_the_script_program_once() {
    let host = make_host(&[(VUE_OWNER, VUE_FIXTURE)]);
    host.provenance().reset();
    let result = host.ensure_indexed_ready(VUE_OWNER);
    assert!(result.is_some(), "the .vue owner must materialise");
    let provenance = snap(&host);
    assert_eq!(
        provenance.vue_script_snapshot_parses, 1,
        "base scheduler-miss lane: the script-program rail counts \
         exactly the worker's single ingress parse — the flight \
         snapshot lane must not add a second parse of the same bytes \
         (got {})",
        provenance.vue_script_snapshot_parses,
    );
    assert_eq!(
        provenance.indexed_ready_scheduler_snapshot_reuse, 1,
        "base scheduler-miss lane: the flight must reuse the scheduler \
         snapshot the ingress worker committed, not rebuild it (got {})",
        provenance.indexed_ready_scheduler_snapshot_reuse,
    );
    assert_eq!(
        provenance.sfc_parses, 1,
        "base scheduler-miss lane: exactly one SFC structure parse per \
         cold .vue build (got {})",
        provenance.sfc_parses,
    );
    assert_eq!(
        provenance.eval_program_parses, 1,
        "base scheduler-miss lane: exactly one eval-program parse per \
         cold .vue build (got {})",
        provenance.eval_program_parses,
    );
    assert_eq!(
        provenance.non_sfc_snapshot_parses, 0,
        "a .vue cold build must never route through the non-SFC \
         snapshot parser (got {})",
        provenance.non_sfc_snapshot_parses,
    );
}

/// Eager upsert lane: the upsert-time `parse_vue_snapshot` funnels
/// through the same single-script-parse snapshot builder.
#[test]
fn vue_upsert_eager_snapshot_parses_the_script_program_once() {
    let host = make_host(&[]);
    host.provenance().reset();
    upsert(&host, VUE_OWNER, VUE_FIXTURE);
    let provenance = snap(&host);
    assert_eq!(
        provenance.vue_script_snapshot_parses, 1,
        "upsert lane: the eager .vue snapshot must OXC-parse the script \
         program exactly once (got {})",
        provenance.vue_script_snapshot_parses,
    );
    assert_eq!(
        provenance.sfc_parses, 1,
        "upsert lane: exactly one SFC structure parse per eager .vue \
         snapshot (got {})",
        provenance.sfc_parses,
    );

    // The subsequent cold materialise reuses the scheduler snapshot —
    // ZERO additional snapshot parses of any kind.
    host.provenance().reset();
    let result = host.ensure_indexed_ready(VUE_OWNER);
    assert!(result.is_some(), "the upserted .vue owner must materialise");
    let provenance = snap(&host);
    assert_eq!(
        provenance.vue_script_snapshot_parses, 0,
        "post-upsert cold materialise must reuse the scheduler snapshot \
         — zero script-program re-parses (got {})",
        provenance.vue_script_snapshot_parses,
    );
    assert_eq!(
        provenance.sfc_parses, 0,
        "post-upsert cold materialise must reuse the scheduler snapshot \
         — zero SFC structure re-parses (got {})",
        provenance.sfc_parses,
    );
}

/// Overlay lane: the overlay materialiser snapshots the OVERLAY
/// content inside its flight (no scheduler snapshot exists for overlay
/// bytes) — same single-script-parse contract as the base lane.
#[test]
fn vue_overlay_cold_flight_parses_the_script_program_once() {
    use crate::session_view::OverlaidView;

    let host = make_host(&[]);
    upsert(&host, VUE_OWNER, VUE_FIXTURE);
    let _ = host
        .ensure_indexed_ready(VUE_OWNER)
        .expect("base .vue owner must materialise");

    let mut overlays: rustc_hash::FxHashMap<String, Arc<str>> = rustc_hash::FxHashMap::default();
    overlays.insert(
        VUE_OWNER.to_string(),
        Arc::from(
            "<script setup lang=\"ts\">export type Q = { b: 2 };\nconst m = 2;\n</script>\n\
             <template><span>{{ m }}</span></template>\n",
        ),
    );
    let view = OverlaidView::new(Arc::clone(&host), overlays);
    host.provenance().reset();
    let result = host.materialize_overlay_indexed_ready_with_view(VUE_OWNER, &view);
    assert!(result.is_some(), "the overlay .vue must materialise");
    let provenance = snap(&host);
    assert_eq!(
        provenance.vue_script_snapshot_parses, 0,
        "overlay lane: the overlay cold flight threads its single \
         eval-program parse into the snapshot build — a snapshot-lane \
         script parse is a second parse of the same overlay bytes \
         (got {})",
        provenance.vue_script_snapshot_parses,
    );
    assert_eq!(
        provenance.sfc_parses, 1,
        "overlay lane: exactly one SFC structure parse of the overlay \
         content per overlay cold flight (got {})",
        provenance.sfc_parses,
    );
}

/// Seed a fresh-stamped artifact-only `IndexedReady` carrying the
/// canonical's REAL source directly into `FileArtifactStore` (no
/// scheduler ingress) — the artifact-only scope `read_analysis_source`
/// serves without loading the canonical into the scheduler.
fn seed_artifact_only_vue(host: &VerterHost, canonical: &str, source: &str) {
    let mut artifact = crate::project_type_store::IndexedReady::new_for_test(crate::hash::hash_16(
        source.as_bytes(),
    ));
    artifact.edge_generation = host.ws().content_generation();
    artifact.project_generation = host.project_type_store().current_project_generation();
    artifact.raw_source = Arc::from(source);
    host.project_type_store()
        .indexed()
        .insert(Arc::from(canonical), Arc::new(artifact));
}

/// Artifact-backed `get_analysis` lane: the canonical is artifact-only
/// (no scheduler ingress), so EVERY scheduler read on the lane misses
/// and the source comes from the retained artifact. One logical
/// `get_analysis` performs exactly ONE SFC structure parse and ONE
/// script-program parse: the snapshot build's parse products are
/// threaded into the template-analysis computation, which must not
/// re-derive them with a second `parse_vue_snapshot` over the same
/// source.
#[test]
fn artifact_backed_get_analysis_parses_the_sfc_once() {
    let host = make_host(&[(VUE_OWNER, VUE_FIXTURE)]);
    seed_artifact_only_vue(&host, VUE_OWNER, VUE_FIXTURE);

    host.provenance().reset();
    let snapshot = host
        .get_analysis(VUE_OWNER)
        .expect("the artifact-backed .vue must produce an analysis snapshot");
    // Anti-vacuity: the read must have stayed artifact-backed. If a
    // future change ingresses the canonical into the scheduler, every
    // scheduler read on the lane starts HITTING and the parse-once
    // pins below stop covering the artifact-only lane at all.
    assert!(
        host.scheduler.try_get_source(VUE_OWNER).is_none(),
        "the artifact-backed read must not ingress the canonical into \
         the scheduler — scheduler presence vacates this test's \
         artifact-only lane coverage",
    );
    // The template analysis must still be computed on this lane — the
    // parse-once contract must not be satisfied by skipping the
    // template computation.
    assert!(
        snapshot.template.is_some(),
        "the artifact-backed read must carry template analysis",
    );
    let provenance = snap(&host);
    assert_eq!(
        provenance.sfc_parses, 1,
        "artifact-backed get_analysis: exactly one SFC structure parse \
         per logical read — the template-analysis computation consumes \
         the snapshot build's parse products instead of re-parsing \
         (got {})",
        provenance.sfc_parses,
    );
    assert_eq!(
        provenance.vue_script_snapshot_parses, 1,
        "artifact-backed get_analysis: exactly one script-program parse \
         per logical read — a second one means the template-analysis \
         computation re-ran the full snapshot parse (got {})",
        provenance.vue_script_snapshot_parses,
    );
}

/// A fatal (recovered-panic) `.vue` script parse performs NO second
/// script-program parse: the flight's single eval-program parse over
/// the extracted script is the only attempt, and the snapshot build
/// defaults the script outputs directly — a re-parse would run over
/// the same bytes under the same source type and is guaranteed to
/// fail identically (the `.vue` mirror of the non-SFC fatal-arm
/// contract pinned by
/// `overlay_fatal_parse_publishes_empty_snapshot_without_second_parse`).
#[test]
fn vue_fatal_script_parse_defaults_outputs_without_second_parse() {
    use crate::session_view::OverlaidView;

    let host = make_host(&[]);
    upsert(&host, VUE_OWNER, VUE_FIXTURE);

    let mut overlays: rustc_hash::FxHashMap<String, Arc<str>> = rustc_hash::FxHashMap::default();
    // Unterminated string literal inside the script block — an
    // unrecoverable (panicked) OXC parse of the extracted script, so
    // `parse_eval_program` yields no program.
    overlays.insert(
        VUE_OWNER.to_string(),
        Arc::from(
            "<script setup lang=\"ts\">const broken = \"unterminated\n</script>\n\
             <template><div /></template>\n",
        ),
    );
    let view = OverlaidView::new(Arc::clone(&host), overlays);
    host.provenance().reset();
    let result = host.materialize_overlay_indexed_ready_with_view(VUE_OWNER, &view);
    let provenance = snap(&host);
    assert_eq!(
        provenance.eval_program_parses, 1,
        "the fatal .vue overlay flight still performs exactly one \
         eval-program parse attempt (got {})",
        provenance.eval_program_parses,
    );
    assert_eq!(
        provenance.vue_script_snapshot_parses, 0,
        "the fatal arm must not re-parse the script on the snapshot \
         lane: same bytes + same source type is a guaranteed identical \
         failure, so the script outputs default directly (got {})",
        provenance.vue_script_snapshot_parses,
    );
    // The fatal flight still publishes an artifact whose script surface
    // is empty — defaulting must not turn the lane into a None.
    let artifact = result.expect("fatal .vue overlay flight still materialises an artifact");
    assert!(
        artifact.shallow_state.symbol("broken").is_none(),
        "a fatally unparseable .vue script carries no shallow symbols",
    );
}

/// Strict-admission empty-facts is a SUPPRESSION signal, not just a
/// RouteDb-local negative-cache pattern — UNROOTABLE-WILDCARD arm. A
/// route entry concluded over an owner whose unresolved `export *`
/// edge cannot be rooted in any `ImportRoute` fact (the owner serves
/// from the artifact-only authority and publishes no import-route
/// surface) is served with EMPTY facts and never persisted by
/// `RouteDb`. But an ENCLOSING traced cold compute (a semantic-memo
/// build, a component-meta proof producer) observes NOTHING from an
/// empty fact list — its own fact stamps validate against the live
/// view while the folded route silently retargets the moment the
/// wildcard target appears, an entry the read-side fact rail cannot
/// reject. The unrootable exit must therefore raise the SAME
/// per-cold-compute suppression flag a fenced serve raises (the
/// `install_fact_tracer` chokepoint every enclosing producer's
/// admission gate consults) — exactly what the route-singleflight
/// follower fallback already does BY HAND for an ADOPTED unrootable
/// route; the leader-produced unrootable route must suppress the same
/// admissions.
#[test]
fn unrootable_wildcard_route_raises_enclosing_cold_compute_suppression() {
    // An UNRESOLVABLE earlier wildcard, then a RESOLVABLE later one:
    // the walk records the unresolved `./missing` edge and still
    // resolves `Shared` through `./present`.
    const BARREL_SOURCE: &str = "export * from './missing';\nexport * from './present';\n";
    let ghost = "/workspace/src/ghost.ts";
    // `ghost.ts` exists in the VFS (so import edges to it resolve) but
    // is NEVER upserted/loaded — the route walk serves it from the
    // artifact-only authority.
    let host = make_host(&[(ghost, BARREL_SOURCE)]);
    let template = "/workspace/src/index.ts";
    let present = "/workspace/src/present.ts";
    upsert(&host, present, "export type Shared = { a: 1 };\n");
    upsert(&host, template, BARREL_SOURCE);

    // Build a REAL barrel artifact from the same content, then seed it
    // for the artifact-only `ghost` canonical with NO import-route
    // surface: the owner shape whose unresolved wildcard edge has no
    // `ImportRoute` fact rail to root on (its wildcards resolve into
    // walk-local edges only — nothing ever publishes `import_routes`
    // for it). Every UPSERTED owner bakes a covering table (the
    // IndexedReady bake records a known-miss entry for every declared
    // specifier), so the artifact-only seed is the one shape that
    // reaches the unrootable arm.
    host.ensure_indexed_ready(template)
        .expect("the template barrel must index");
    let template_hash = host.get_whole_hash(template).expect("template tracked");
    let baked = host
        .project_type_store()
        .indexed()
        .get(template, template_hash)
        .expect("the baked template artifact");
    let mut unrooted = (*baked).clone();
    unrooted.import_routes = Arc::new(rustc_hash::FxHashMap::default());
    unrooted.edge_generation = host.ws().content_generation();
    unrooted.project_generation = host.project_type_store().current_project_generation();
    host.project_type_store()
        .indexed()
        .insert(Arc::from(ghost), Arc::new(unrooted));

    // The producer-level observable: run the route-entry build inside
    // an installed fact tracer — the SAME wrapper every enclosing cold
    // compute (semantic-memo builds, the owner-import-surface and
    // component-meta proof producers) installs around its cold body —
    // and read the chokepoint flag its admission gates consult.
    let (entry, _finalise, suppression_raised) =
        crate::fact_signature_helpers::install_fact_tracer(&host, || {
            host.build_named_type_export_route_entry(ghost, "Shared")
        });
    let (route, facts) = entry.expect("the route must resolve through the later wildcard");
    assert!(
        matches!(route, crate::resolver_core::RouteResult::Resolved { .. }),
        "the unrootable walk still serves its caller the resolved root \
         (got {route:?})",
    );
    assert!(
        facts.is_empty(),
        "precondition: the unresolved ./missing edge cannot be rooted — \
         the entry is served with the empty-facts strict-admission signal",
    );

    // The discriminator: the unrootable exit must raise the enclosing
    // cold-compute suppression flag. Pre-fix the exit returned empty
    // facts SILENTLY — an enclosing producer published a value folding
    // a route none of its recorded facts can root.
    assert!(
        suppression_raised,
        "an unrootable empty-facts route exit must raise the enclosing \
         cold-compute suppression chokepoint (the install_fact_tracer \
         flag every shared-cache admission gate consults) — a silent \
         empty-facts return lets an enclosing producer publish a value \
         that stale-serves once the unresolved wildcard target appears",
    );
}

/// Negative control for the unrootable-wildcard suppression arm: a
/// route entry whose walk roots EVERY unresolved wildcard edge (the
/// owner's baked import-route table covers the missing source as a
/// known-miss) must NOT raise the suppression flag — the entry carries
/// real facts and enclosing producers admit normally. Proves the
/// unrootable exit's marking keys on rootability, not on wildcard
/// misses in general.
#[test]
fn rooted_wildcard_route_does_not_raise_enclosing_suppression() {
    let host = make_host(&[]);
    let barrel = "/workspace/src/index.ts";
    let present = "/workspace/src/present.ts";
    upsert(&host, present, "export type Shared = { a: 1 };\n");
    upsert(
        &host,
        barrel,
        "export * from './missing';\nexport * from './present';\n",
    );

    let (entry, _finalise, suppression_raised) =
        crate::fact_signature_helpers::install_fact_tracer(&host, || {
            host.build_named_type_export_route_entry(barrel, "Shared")
        });
    let (route, facts) = entry.expect("the route must resolve through the later wildcard");
    assert!(
        matches!(route, crate::resolver_core::RouteResult::Resolved { .. }),
        "Shared resolves through ./present (got {route:?})",
    );
    assert!(
        !facts.is_empty(),
        "an upserted barrel's baked table roots the unresolved wildcard \
         (known-miss entry) — the entry carries real facts",
    );
    assert!(
        !suppression_raised,
        "a fully-rooted route must not raise the suppression chokepoint \
         — enclosing producers admit normally (over-decline would refuse \
         every wildcard-bearing barrel)",
    );
}

/// Strict-admission refusal is a SUPPRESSION signal, not just a
/// producer-local non-admission — OWNER-IMPORT-SURFACE UNROOTED-SKIP
/// arm. A surface built over an owner with an unresolvable direct
/// import (the specifier is SKIPPED) that cannot be rooted in the
/// owner's `ImportRoute` fact rail (the owner serves from the
/// artifact-only authority and publishes no import-route surface) is
/// served to the caller and refused its OWN warm admission. But an
/// ENCLOSING traced cold compute (a semantic-memo build, a
/// component-meta proof producer) observes NOTHING from the refusal —
/// no route walk runs for the skipped specifier, the canonical-resolve
/// miss records no tracer fact, and the enclosing producer's own fact
/// stamps validate against the live view while the consumed surface
/// (computed WITHOUT the import — e.g. "this binding is not an
/// imported root") silently retargets the moment the missing target
/// appears: the owner's whole hash does not move, so no recorded fact
/// can reject the entry. The unrooted-skip exit must therefore raise
/// the SAME per-cold-compute suppression flag a fenced serve raises
/// (the `install_fact_tracer` chokepoint every enclosing producer's
/// admission gate consults) — exactly what the unrootable-wildcard
/// route exit already does for the route-walk shape of the same hole.
#[test]
fn unrooted_import_skip_raises_enclosing_cold_compute_suppression() {
    // An UNRESOLVABLE direct import: the surface build skips the
    // `Missing` binding and records `./missing` as an unresolved
    // source that must be rooted in the owner's `ImportRoute` rail.
    const OWNER_SOURCE: &str =
        "import type { Missing } from './missing';\nexport type Uses = Missing;\n";
    let ghost = "/workspace/src/ghost.ts";
    // `ghost.ts` exists in the VFS (so its content pins) but is NEVER
    // upserted/loaded — the surface build serves it from the
    // artifact-only authority.
    let host = make_host(&[(ghost, OWNER_SOURCE)]);
    let template = "/workspace/src/index.ts";
    upsert(&host, template, OWNER_SOURCE);

    // Build a REAL artifact from the same content, then seed it for
    // the artifact-only `ghost` canonical with NO import-route
    // surface: the owner shape whose skipped specifier has no
    // `ImportRoute` fact rail to root on. Every UPSERTED owner bakes a
    // covering table (the IndexedReady bake records a known-miss entry
    // for every declared specifier), so the artifact-only seed is the
    // one shape that reaches the unrooted-skip arm.
    host.ensure_indexed_ready(template)
        .expect("the template owner must index");
    let template_hash = host.get_whole_hash(template).expect("template tracked");
    let baked = host
        .project_type_store()
        .indexed()
        .get(template, template_hash)
        .expect("the baked template artifact");
    let mut unrooted = (*baked).clone();
    unrooted.import_routes = Arc::new(rustc_hash::FxHashMap::default());
    unrooted.edge_generation = host.ws().content_generation();
    unrooted.project_generation = host.project_type_store().current_project_generation();
    host.project_type_store()
        .indexed()
        .insert(Arc::from(ghost), Arc::new(unrooted));

    // The producer-level observable: run the surface build inside an
    // installed fact tracer — the SAME wrapper every enclosing cold
    // compute (semantic-memo builds, the component-meta proof
    // producers) installs around its cold body — and read the
    // chokepoint flag its admission gates consult.
    let before = snap(&host).owner_import_surface_unrooted_skip_refusals;
    let (surface, _finalise, suppression_raised) =
        crate::fact_signature_helpers::install_fact_tracer(&host, || {
            host.owner_import_surface(ghost)
        });
    let surface = surface.expect("the unrooted build still serves its caller the surface");
    assert_eq!(
        snap(&host).owner_import_surface_unrooted_skip_refusals,
        before + 1,
        "precondition: the skipped ./missing specifier cannot be rooted \
         — the build takes the unrooted-skip refusal arm",
    );
    assert!(
        host.project_type_store()
            .owner_import_surfaces()
            .get(ghost, surface.owner_whole_hash)
            .is_none(),
        "precondition: the unrooted surface is served, never persisted \
         (refused its own warm admission)",
    );

    // The discriminator: the unrooted-skip exit must raise the
    // enclosing cold-compute suppression flag. Pre-fix the arm refused
    // only its OWN admission and returned SILENTLY — an enclosing
    // producer published a value folding a surface none of its
    // recorded facts can root.
    assert!(
        suppression_raised,
        "an unrooted-skip surface refusal must raise the enclosing \
         cold-compute suppression chokepoint (the install_fact_tracer \
         flag every shared-cache admission gate consults) — a silent \
         refusal lets an enclosing producer publish a value that \
         stale-serves once the skipped import target appears",
    );
}

/// Negative control for the unrooted-skip suppression arm: a surface
/// whose skipped specifier IS rooted in the owner's `ImportRoute` fact
/// rail (the upserted owner's baked table covers the missing source as
/// a known-miss) must NOT raise the suppression flag — the surface
/// carries the rooting fact, admits warm, and enclosing producers
/// admit normally. Proves the marking keys on rootability, not on
/// skip-ness: over-decline would refuse every owner with an
/// unresolvable import.
#[test]
fn rooted_import_skip_does_not_raise_enclosing_suppression() {
    let host = make_host(&[]);
    let owner = "/workspace/src/index.ts";
    upsert(
        &host,
        owner,
        "import type { Missing } from './missing';\nexport type Uses = Missing;\n",
    );

    let before = snap(&host).owner_import_surface_unrooted_skip_refusals;
    let (surface, _finalise, suppression_raised) =
        crate::fact_signature_helpers::install_fact_tracer(&host, || {
            host.owner_import_surface(owner)
        });
    let surface = surface.expect("the rooted build serves the surface");
    assert!(
        surface.bindings.get("Missing").is_none(),
        "precondition: the unresolvable ./missing import is SKIPPED — \
         the surface computes without the binding",
    );
    assert_eq!(
        snap(&host).owner_import_surface_unrooted_skip_refusals,
        before,
        "an upserted owner's baked table covers the skipped specifier \
         (known-miss entry) — the refusal arm never fires",
    );
    assert!(
        surface.read_set_signature.facts.iter().any(|fact| matches!(
            fact,
            crate::resolver_core::FactVersionRef::DerivedFactHash {
                canonical_id,
                kind: crate::resolver_core::DerivedFactKind::ImportRoute,
                ..
            } if canonical_id == owner
        )),
        "the skipped specifier is rooted in the owner's ImportRoute \
         fact — the rail that moves when the missing target appears",
    );
    assert!(
        host.project_type_store()
            .owner_import_surfaces()
            .get(owner, surface.owner_whole_hash)
            .is_some(),
        "a rooted surface admits warm — the surface cache persists it",
    );
    assert!(
        !suppression_raised,
        "a fully-rooted skip must not raise the suppression chokepoint \
         — enclosing producers admit normally (over-decline would \
         refuse every owner with an unresolvable import)",
    );
}
