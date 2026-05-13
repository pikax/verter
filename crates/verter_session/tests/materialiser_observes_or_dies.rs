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

    // The materialiser entry struct MUST carry a `dep_signature`
    // field. A refactor that dropped it would mean cached entries
    // had no observed facts to validate against — i.e., the cache
    // could not detect staleness.
    assert!(
        src.contains("dep_signature: DepSignature"),
        "MaterializeStructureEntry must carry a `dep_signature: DepSignature` field \
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

/// Sub-task E: every dispatch / sub-query in the materialiser cold-
/// compute path MUST observe its consumed signature onto the active
/// fact-read tracer via `observe_dep_signature` / `observe_fence_entry`.
///
/// The arch-guard counts observe call sites in the materialiser and
/// requires they cover every dispatch-family + every fence-push site.
/// A regression below the threshold means a dispatch family or a
/// scope-canonical seed push is missing its observe pairing — the
/// fact-based tracer would record an incomplete signature and the
/// strict admission guard would either refuse the resulting cache
/// candidate (`admission_refused_count` increments) or admit a
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

    // Discrimination 2: at least one `observe_fence_entry` per
    // scope-canonical seed push (`finish_cacheable` + the in-place
    // closure seed at the end of `materialize_component_meta_structure`).
    let observe_fence_count = src.matches("observe_fence_entry(").count();
    assert!(
        observe_fence_count >= 2,
        "materialiser must observe scope-canonical seed pushes via \
         `observe_fence_entry(...)` in both `finish_cacheable` AND \
         the cold-compute closure tail. Got {} call sites — a value \
         below 2 means the scope-canonical whole_hash dependency is \
         not recorded on the active fact-read tracer; downstream \
         staleness signals would miss owner-file edits.",
        observe_fence_count
    );
}
