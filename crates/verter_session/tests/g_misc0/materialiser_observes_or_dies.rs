//! Sub-task L arch guard — materialiser cache lookups MUST be
//! paired with `observe(...)` calls.
//!
//! Stage 6d's arch-guard contract (plan §783-805):
//!
//! > Every materialiser cache lookup observes the corresponding
//! > `Member` / `MemberPresence` / `MemberShape` / `LocalDecl` /
//! > `ImportRef` / `ResolvedImportClause` / route fact /
//! > `ModuleAugmentationIndexShape` (when augmented surface is
//! > consumed). **Arch-guard**: materialiser code MUST NOT read a
//! > cache value without recording fact dependencies.
//!
//! The grep target is the production materialiser surface
//! (`crates/verter_session/src/component_meta_materialize.rs` +
//! `crates/verter_session/src/component_meta_caches.rs` +
//! `crates/verter_session/src/meta_resolve/`). The guard scans the
//! source for cache-read patterns that should be paired with an
//! `observe` / fact-merge call.
//!
//! **Discrimination.** Stage 6d wires the materialiser to consume
//! observed signatures via `merge_dep_signature_into_local_fence`
//! (the existing fence-merge path used by route + member reads).
//! A future refactor that introduces a `cache.peek(...)` call
//! WITHOUT a paired `merge_dep_signature_into_local_fence(...)` or
//! `local_fence.push(...)` would slip past this guard — caught by
//! a future Stage 7 follow-up that promotes the grep into a
//! semantic dependence check. For Stage 6d we pin the architectural
//! intent + the locations where observe-pairing already lives.

use std::fs;
use std::path::PathBuf;

fn crates_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .to_path_buf()
}

fn read(path: &PathBuf) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| panic!("read {path:?}: {e}"))
}

/// The materialiser pipeline (`component_meta_materialize.rs`) MUST
/// thread its observed dep-signature through the local fence on
/// every dispatch. The discriminating signal is the presence of
/// `merge_dep_signature_into_local_fence(...)` next to every
/// `dispatch.execute_read(...)` call.
#[test]
fn materialiser_pipeline_merges_dep_signature_after_every_dispatch() {
    let path = crates_dir()
        .join("verter_session")
        .join("src")
        .join("component_meta_materialize.rs");
    let src = read(&path);

    // Discrimination 1: a real materialiser MUST call
    // `merge_dep_signature_into_local_fence` at least once per
    // dispatch family (route / pick / omit / indexed access /
    // recursive helper). The Stage 0 baseline carries multiple
    // call sites; we require at least one per family.
    let merge_count = src.matches("merge_dep_signature_into_local_fence").count();
    assert!(
        merge_count >= 5,
        "materialiser must merge observed dep-signatures into the local fence on every \
         dispatch family (route, pick, omit, indexed-access, recursive-helper). Got \
         {} call sites — a regression below 5 means a dispatch family is forgetting \
         to thread its observed facts back into the fence (arch-guard violation).",
        merge_count
    );

    // Discrimination 2: the materialiser MUST call
    // `dispatch.execute_read(...)` to consume sub-queries. A
    // refactor that bypassed dispatch (e.g. private drill-down)
    // would slip past the fence-merge guard.
    let exec_count = src.matches(".execute_read(").count();
    assert!(
        exec_count >= 3,
        "materialiser must consume sub-queries via dispatch.execute_read(). Got {} \
         call sites — a value < 3 means the materialiser took a private drill-down \
         path that bypasses the shared query memo + fence-merge (arch-guard \
         violation).",
        exec_count
    );
}

/// The materialiser cache (`component_meta_caches.rs::MaterializeStructureDb`)
/// MUST store the observed dep-signature on every cached entry.
/// The discriminating signal is the presence of `dep_signature` on
/// `MaterializeStructureEntry`.
#[test]
fn materialise_structure_entry_carries_dep_signature() {
    let path = crates_dir()
        .join("verter_session")
        .join("src")
        .join("component_meta_caches.rs");
    let src = read(&path);

    // The materialiser entry struct MUST carry the carrier
    // `read_set_signature: ReadSetSignature` field. The carrier
    // consolidates the legacy whole-hash rail and the path-precise
    // fact-signature rail; cache lookups validate through
    // `entry.read_set_signature.validate_with_self_roots(ctx,
    // &entry.self_root_canonicals)`. A refactor that dropped the
    // carrier would mean cached entries had no observed facts to
    // validate against — i.e., the cache could not detect staleness.
    assert!(
        src.contains("read_set_signature: crate::fact_signature_helpers::ReadSetSignature")
            || src.contains("read_set_signature: ReadSetSignature"),
        "MaterializeStructureEntry must carry a `read_set_signature: ReadSetSignature` field \
         so cache lookups can re-validate. The arch-guard pins this field's presence; \
         dropping it breaks the fact-validation contract."
    );

    // The reverse index must hold the per-canonical drain set so
    // `invalidate_for_canonical` can find every entry the
    // canonical participates in.
    assert!(
        src.contains("canonical_to_keys"),
        "MaterializeStructureDb must carry the per-canonical reverse-index drain map. \
         The arch-guard pins this field; dropping it would break per-canonical \
         invalidation."
    );
}

/// RouteDb consumers MUST emit fact-version refs into the
/// per-candidate `fact_dep_signature`.
#[test]
fn route_db_writes_record_fact_dep_signatures() {
    let path = crates_dir()
        .join("verter_session")
        .join("src")
        .join("resolver_core")
        .join("route_db.rs");
    let src = read(&path);

    // RouteDb writes MUST include the `fact_dep_signature` field on
    // every produced surface. A refactor that omitted the field
    // would mean route entries published with no observed facts —
    // an arch-guard violation.
    assert!(
        src.contains("fact_dep_signature"),
        "RouteDb writes must record fact-version refs as the per-candidate \
         `fact_dep_signature`. Arch-guard violation: route entries with no observed \
         facts cannot revalidate."
    );
}

/// Every dispatch / sub-query in the materialiser cold-compute path
/// MUST observe its consumed signature onto the active fact-read
/// tracer, and the resulting cache entry MUST root on its observed
/// facts.
///
/// The arch-guard counts call sites in the materialiser. The three
/// discriminations differ in strength:
///
/// - **Discrimination 1** (`observe_dep_signature` `>= 6`) is a
///   per-dispatch-family floor: there is one expected `observe_dep_signature`
///   site per dispatch family, so a count below 6 means a family
///   dropped its observe pairing.
/// - **Discriminations 2 and 3** (`install_fact_tracer`,
///   `merge_traced_facts_into_materialize_carrier`,
///   `base_node_origin_self_root`, each `>= 1`) are
///   wiring-existence floors: each names a single cold-compute wiring
///   point, so the `>= 1` threshold catches **total deletion** of
///   that wiring — not a per-family gap. They are intentionally not
///   exact-count assertions: a legitimate refactor may add or fold
///   call sites, and an exact count would break the guard
///   spuriously.
///
/// A regression that trips any check means the fact-based tracer
/// would record an incomplete signature and the strict admission
/// guard would either refuse the resulting cache candidate or admit a
/// candidate that misses some of its real dependencies (silent
/// staleness on later edits).
#[test]
fn materialiser_pipeline_observes_dep_signatures_on_every_dispatch_family() {
    let path = crates_dir()
        .join("verter_session")
        .join("src")
        .join("component_meta_materialize.rs");
    let src = read(&path);

    // Discrimination 1: at least one `observe_dep_signature` per
    // dispatch family (Pick/Omit body, Pick/Omit projected,
    // recursive Instantiate body, ProjectPath fallback, sub-query
    // via `materialize_child_at_nested`). Counting the call sites
    // pins each family.
    let observe_dep_count = src.matches("observe_dep_signature(").count();
    assert!(
        observe_dep_count >= 6,
        "materialiser must observe sub-query dep signatures via \
         `observe_dep_signature(ctx, ...)` at every dispatch family \
         (Pick/Omit body, Pick/Omit projected, Instantiate body, \
         recursive body materialise, ProjectPath fallback, \
         materialize_child_at_nested). Got {} call sites — a value \
         below 6 means a dispatch family is missing its observe \
         pairing; the resulting cold compute would record an \
         incomplete fact-dep signature.",
        observe_dep_count
    );

    // Discrimination 2: the materialiser cold compute MUST run under an
    // `install_fact_tracer` scope and re-base the published carrier on
    // that scope's authoritative observation set via
    // `merge_traced_facts_into_materialize_carrier`. The tracer scope is
    // what records every transitively-bubbled fact the materialiser's
    // legacy `local_fence` (whole-hash rails only) can miss; the merge
    // folds the traced set onto the producer carrier's observed
    // self-roots. A `MaterializeStructureDb` entry's identity does NOT
    // depend on the consumer materialise scope (R7 cross-owner reuse),
    // so the carrier is rooted on the `base` node's declaration-origin
    // self-root plus the traced facts — NOT on a synthetic scope seed.
    // Dropping the tracer scope or the carrier re-base would publish an
    // entry whose signature misses its real dependencies.
    let install_tracer_count = src.matches("install_fact_tracer").count();
    assert!(
        install_tracer_count >= 1,
        "the materialiser cold compute MUST run under an \
         `install_fact_tracer` scope so every transitively-bubbled fact \
         is recorded on the fact-read tracer. Got {} call sites — zero \
         means the cold compute observes no traced facts and the \
         published cache entry would miss its real dependencies.",
        install_tracer_count
    );
    let merge_traced_count = src
        .matches("merge_traced_facts_into_materialize_carrier(")
        .count();
    assert!(
        merge_traced_count >= 1,
        "the materialiser MUST re-base the published `MaterializeStructureDb` carrier \
         on the `install_fact_tracer` observation set via \
         `merge_traced_facts_into_materialize_carrier(...)`. Got {} call sites — zero \
         means the cold-compute carrier is not folded onto the traced facts, so the \
         entry's signature would miss transitively-observed dependencies and serve \
         stale after an edit to one of them.",
        merge_traced_count
    );

    // Discrimination 3: the materialiser MUST root each entry on the
    // `base` node's declaration-origin self-root — the load-bearing
    // identity that replaced the (non-load-bearing) consumer-scope seed.
    let base_origin_count = src.matches("base_node_origin_self_root").count();
    assert!(
        base_origin_count >= 1,
        "the materialiser MUST root each `MaterializeStructureDb` entry on the `base` \
         node's declaration-origin file via `base_node_origin_self_root(...)`. Got {} \
         call sites — zero means the entry has no strict self-root and a content edit \
         to the base declaration's file could not invalidate it.",
        base_origin_count
    );
}
