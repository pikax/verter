//! Self-version-root discriminator tests for the nine component-meta
//! query-identity caches.
//!
//! Each query-identity cache is keyed by a canonical (the entry's
//! *keyed canonical*). The warm-read validator must validate the
//! entry's self-root `FileWholeHash` for that keyed canonical
//! **strictly** — through
//! [`crate::fact_signature_helpers::validate_fact_signature_with_self_roots`]
//! — so a keyed canonical that is untracked by the live store view
//! rejects the entry instead of riding the lazy
//! [`crate::resolver_core::StoreView::validates`] "untracked file →
//! optimistically accept" rule.
//!
//! ## Discrimination model
//!
//! The central fact-signature helpers already prepend a self-root
//! `FileWholeHash` for the keyed canonical. The lazy
//! [`crate::resolver_core::StoreView::validates`] already rejects a
//! **tracked** `FileWholeHash` whose hash mismatches current content
//! (`Some(current) => current == hash`). So a same-canonical content
//! edit where the file stays tracked is already detected by the lazy
//! validator — that path is *not* a discriminator for the strict
//! warm-read change.
//!
//! The behavior the strict self-root validator adds is the
//! **untracked** keyed-canonical case: lazy `validates` returns `true`
//! for an untracked `FileWholeHash`; strict
//! `validates_self_root_whole_hash` returns `false`. Both
//! `cooperative_get_or_insert` paths consult the same validator — the
//! warm-hit `validate` closure AND the post-compute
//! `revalidate_after_compute` closure. Every `*_untracked_self_root_*`
//! test below drives exactly this: a first `get_or_compute` call's
//! cold closure returns a "stale" value paired with a self-root
//! `FileWholeHash` for a canonical the live store view does not track,
//! then a second `get_or_compute` issues the warm read.
//!
//! - With a **lazy** self-root validator (the untracked-accept arm of
//!   `StoreView::validates`), the first call's `revalidate_after_compute`
//!   accepts the untracked self-root, so the "stale" entry is ADMITTED.
//!   The second call is then a warm hit: it returns the stale value
//!   and its cold closure never runs.
//! - With the **strict** self-root validator, the first call's
//!   `revalidate_after_compute` rejects the untracked self-root, so the
//!   "stale" entry is NOT admitted. The second call finds no entry, so
//!   its cold closure runs and the recomputed value surfaces.
//!
//! Each test asserts the second call's cold closure ran AND the
//! recomputed (not the stale) value surfaced — discriminating against
//! any tree whose self-root validation is lazy.
//!
//! The two `*_skip_own_drain_*` tests at the end cover the producer
//! widening for `PreparedTargetDb` (the declaring canonical is a
//! second self-root) and `MaterializeMemoDb` (every canonical observed
//! during materialization is a dependency fact): they prime a warm
//! entry, edit a *secondary* (non-keyed-scope) canonical through
//! [`crate::VerterHost::upsert_skipping_own_canonical_drain_for_tests`]
//! so the own-canonical drain does not mask the staleness, and assert
//! the warm read misses. Pre-widening the producer recorded no fact
//! for the secondary canonical so the entry validated stale.

use std::collections::BTreeSet;
use std::sync::Arc;

use verter_semantic::analysis::type_solver::query_engine::ProjectedMember;
use verter_type_expr::TypeExpr;

use crate::fact_signature_helpers::empty_fact_signature;
use crate::project_semantic_dispatch::raise::MaterializedTypeExpr;
use crate::resolver_core::cache_keys::{
    PreparedMemberCacheKey, PreparedMemberCacheKind, PreparedSubstitutionKey,
    PreparedSurfaceCacheKey, PreparedTargetCacheKey, RoutedExprSurfaceCacheKey,
};
use crate::resolver_core::component_meta_query_engine::ResolvedImportedRegistrySymbol;
use crate::resolver_core::{
    FactVersionRef, MaterializeScopeObservation, ResolvedDeclarationKind, ResolvedTypeDeclaration,
    ResolverContext, RouteDemand, StoreView,
};
use crate::{HostConfig, UpsertRequest, VerterHost};

/// A self-root `FileWholeHash` byte pattern for a planted (untracked)
/// entry. Distinct from any real content hash.
const PLANTED_HASH: [u8; 16] = [0xAB; 16];

/// Upsert through the production path.
fn upsert(host: &VerterHost, path: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(path.to_string()),
            input_id: path.to_string(),
            source: Arc::from(source),
            file_kind: crate::FileKind::from_path(path),
            aliases: Vec::new(),
        })
        .expect("upsert succeeds");
}

/// Upsert through the test-only skip-own-canonical-drain hook so the
/// upserted canonical's own query-identity cache entries are NOT
/// drained.
fn upsert_skip_drain(host: &VerterHost, path: &str, source: &str) {
    let _ = host
        .upsert_skipping_own_canonical_drain_for_tests(UpsertRequest {
            canonical_id: Some(path.to_string()),
            input_id: path.to_string(),
            source: Arc::from(source),
            file_kind: crate::FileKind::from_path(path),
            aliases: Vec::new(),
        })
        .expect("skip-drain upsert succeeds");
}

/// A standalone host with one unrelated `.ts` file materialized, so a
/// live store view exists but the probe canonical (never loaded) is
/// untracked.
fn host_with_unrelated_file() -> VerterHost {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert(
        &host,
        "/self_root_qdb/unrelated.ts",
        "export const anchor = 1;\n",
    );
    host.ensure_indexed_ready("/self_root_qdb/unrelated.ts")
        .expect("unrelated IndexedReady materialises");
    host
}

/// A one-fact signature whose sole entry is a self-root
/// `FileWholeHash` for `canonical` at [`PLANTED_HASH`].
fn planted_self_root(canonical: &str) -> Arc<[FactVersionRef]> {
    Arc::from(vec![FactVersionRef::FileWholeHash {
        canonical_id: canonical.to_string(),
        hash: PLANTED_HASH,
    }])
}

/// Assert the probe canonical is genuinely untracked by the live
/// store view — otherwise the lazy and strict arms are
/// indistinguishable and the test would not discriminate.
fn assert_untracked(host: &VerterHost, canonical: &str) {
    let ctx: &dyn ResolverContext = host;
    let view = ctx.resolver_store_view();
    assert!(
        !StoreView::tracks_file(&view, canonical),
        "fixture invariant: probe canonical {canonical} must be UNTRACKED by the live \
         store view — a tracked canonical makes lazy and strict validation identical",
    );
}

fn decl(text: &str, canonical: &str) -> ResolvedTypeDeclaration {
    ResolvedTypeDeclaration {
        requested_name: "Probe".to_string(),
        declaration_id: None,
        resolved_name: "Probe".to_string(),
        canonical_source: canonical.to_string(),
        span: verter_span::Span::default(),
        kind: ResolvedDeclarationKind::Interface,
        text: Some(text.to_string()),
    }
}

// ---------------------------------------------------------------------------
// Item 2 — DeclarationLookupDb.
// ---------------------------------------------------------------------------

/// `DeclarationLookupDb` validates the keyed canonical's self-root
/// strictly: an entry whose keyed canonical is untracked is neither
/// admitted at cold-publish time nor served on a warm read.
///
/// Discriminating property: the first `get_or_compute`'s cold closure
/// pairs a `"stale"` declaration with a single self-root
/// `FileWholeHash` for an untracked canonical. A lazy self-root
/// validator admits the entry (`revalidate_after_compute` accepts the
/// untracked self-root), so the second `get_or_compute` is a warm hit
/// — its cold closure never runs and `"stale"` surfaces. The strict
/// validator rejects admission, so the second call recomputes and
/// `"recomputed"` surfaces.
#[test]
fn declaration_lookup_db_untracked_self_root_rejects_warm_entry() {
    let host = host_with_unrelated_file();
    let c = "/self_root_qdb/decl_never_loaded.ts";
    assert_untracked(&host, c);
    let ctx: &dyn ResolverContext = &host;
    let db = host.project_type_store().declaration_db();
    let key = (Arc::<str>::from(c), Arc::<str>::from("Probe"));

    // Prime attempt: a "stale" value paired with an untracked
    // self-root. A lazy validator admits this; the strict one does not.
    let _ = db.get_or_compute(&key, ctx, || Some((decl("stale", c), planted_self_root(c))));

    let mut cold_ran = false;
    let warm = db
        .get_or_compute(&key, ctx, || {
            cold_ran = true;
            Some((decl("recomputed", c), empty_fact_signature()))
        })
        .expect("warm path produces a value");

    assert!(
        cold_ran,
        "DeclarationLookupDb MUST NOT serve a warm entry whose self-root FileWholeHash \
         names an untracked keyed canonical — a lazy self-root validator admits the \
         untracked-self-root entry and the warm read returns it stale without \
         recomputing",
    );
    assert_eq!(
        warm.text.as_deref(),
        Some("recomputed"),
        "the rejected entry must NOT bubble its stale value — the recomputed \
         declaration must surface",
    );
}

// ---------------------------------------------------------------------------
// Item 3 — ImportedRegistryDb.
// ---------------------------------------------------------------------------

fn imported_symbol(canonical: &str, marker: &str) -> ResolvedImportedRegistrySymbol {
    ResolvedImportedRegistrySymbol {
        canonical_id: canonical.to_string(),
        exported_name: marker.to_string(),
        body: TypeExpr::Unknown {
            raw: marker.to_string(),
        },
        canonical_dependencies: BTreeSet::new(),
    }
}

/// `ImportedRegistryDb` validates the keyed canonical's self-root
/// strictly.
///
/// Discriminating property: identical shape to the
/// `DeclarationLookupDb` test — an untracked-self-root entry is
/// admitted (and then served stale) by a lazy validator and rejected
/// by the strict validator.
#[test]
fn imported_registry_db_untracked_self_root_rejects_warm_entry() {
    let host = host_with_unrelated_file();
    let c = "/self_root_qdb/imported_never_loaded.ts";
    assert_untracked(&host, c);
    let ctx: &dyn ResolverContext = &host;
    let db = host.project_type_store().imported_registry_db();
    let key = (Arc::<str>::from(c), Arc::<str>::from("Probe"));

    let _ = db.get_or_compute_admit(&key, ctx, || {
        crate::cooperative_admission::ComputeAdmission::Cacheable(
            crate::component_meta_caches::ImportedRegistryEntry {
                value: Some(Arc::new(imported_symbol(c, "stale"))),
                fact_dep_signature: planted_self_root(c),
            },
        )
    });

    let mut cold_ran = false;
    let warm = db
        .get_or_compute_admit(&key, ctx, || {
            cold_ran = true;
            crate::cooperative_admission::ComputeAdmission::Cacheable(
                crate::component_meta_caches::ImportedRegistryEntry {
                    value: Some(Arc::new(imported_symbol(c, "recomputed"))),
                    fact_dep_signature: empty_fact_signature(),
                },
            )
        })
        .expect("warm path produces a value");

    assert!(
        cold_ran,
        "ImportedRegistryDb MUST NOT serve a warm entry whose self-root names an \
         untracked keyed canonical",
    );
    assert_eq!(
        warm.map(|s| s.exported_name.clone()),
        Some("recomputed".to_string()),
        "the rejected entry must not bubble its stale resolved symbol",
    );
}

// ---------------------------------------------------------------------------
// Item 4 — ResolvabilityDb.
// ---------------------------------------------------------------------------

/// `ResolvabilityDb` validates the keyed canonical's self-root
/// strictly.
///
/// Discriminating property: the prime attempt's value is `false`; the
/// recompute produces `true`. A lazy validator admits the `false`
/// entry and the warm read returns it; the strict validator rejects
/// admission, so the recomputed `true` surfaces — a directly
/// observable boolean flip.
#[test]
fn resolvability_db_untracked_self_root_rejects_warm_entry() {
    let host = host_with_unrelated_file();
    let c = "/self_root_qdb/resolvable_never_loaded.ts";
    assert_untracked(&host, c);
    let ctx: &dyn ResolverContext = &host;
    let db = host.project_type_store().resolvable_db();
    let key = (Arc::<str>::from(c), Arc::<str>::from("Probe"));

    let _ = db.get_or_compute(&key, ctx, || Some((false, planted_self_root(c))));

    let mut cold_ran = false;
    let warm = db
        .get_or_compute(&key, ctx, || {
            cold_ran = true;
            Some((true, empty_fact_signature()))
        })
        .expect("warm path produces a value");

    assert!(
        cold_ran,
        "ResolvabilityDb MUST NOT serve a warm entry whose self-root names an \
         untracked keyed canonical",
    );
    assert!(
        warm,
        "the rejected entry must not bubble its stale `false` — the recomputed \
         `true` must surface",
    );
}

// ---------------------------------------------------------------------------
// Item 5 — OwnerCollectionDb.
// ---------------------------------------------------------------------------

/// `OwnerCollectionDb` validates the keyed owner canonical's self-root
/// strictly. This cache is body-bearing (stores a `TypeExpr`), so
/// strict self-root validation is the correctness floor.
///
/// Discriminating property: the prime attempt stores a `TypeExpr`
/// carrying the marker `"stale"`; the recompute stores `"recomputed"`.
/// A lazy validator admits the stale body and the warm read returns
/// it; the strict validator rejects admission.
#[test]
fn owner_collection_db_untracked_self_root_rejects_warm_entry() {
    let host = host_with_unrelated_file();
    let c = "/self_root_qdb/owner_never_loaded.ts";
    assert_untracked(&host, c);
    let ctx: &dyn ResolverContext = &host;
    let db = host.project_type_store().owner_collection_db();
    let key = (Arc::<str>::from(c), Arc::<str>::from("Probe"));

    let _ = db.get_or_compute(&key, ctx, || {
        Some((
            Some(TypeExpr::Unknown {
                raw: "stale".to_string(),
            }),
            planted_self_root(c),
        ))
    });

    let mut cold_ran = false;
    let warm = db
        .get_or_compute(&key, ctx, || {
            cold_ran = true;
            Some((
                Some(TypeExpr::Unknown {
                    raw: "recomputed".to_string(),
                }),
                empty_fact_signature(),
            ))
        })
        .expect("warm path produces a value");

    assert!(
        cold_ran,
        "OwnerCollectionDb MUST NOT serve a warm entry whose self-root names an \
         untracked keyed canonical",
    );
    assert!(
        matches!(warm.as_deref(), Some(TypeExpr::Unknown { raw }) if raw == "recomputed"),
        "the rejected entry must not bubble its stale body expression",
    );
}

// ---------------------------------------------------------------------------
// Item 6 — PreparedTargetDb.
// ---------------------------------------------------------------------------

fn prepared_target_key(active_scope: &str, decl_canonical: &str) -> PreparedTargetCacheKey {
    PreparedTargetCacheKey {
        active_scope_canonical_id: Arc::from(active_scope),
        decl_canonical_id: Arc::from(decl_canonical),
        decl_symbol_name: Arc::from("Probe"),
        requested_name: Arc::from("Probe"),
    }
}

/// `PreparedTargetDb` validates BOTH the active-scope and the
/// declaring canonical as self-roots.
///
/// Discriminating property: an untracked self-root for the active
/// scope is admitted (and served stale) by a lazy validator and
/// rejected by the strict one.
#[test]
fn prepared_target_db_untracked_self_root_rejects_warm_entry() {
    let host = host_with_unrelated_file();
    let scope = "/self_root_qdb/ptgt_never_loaded.ts";
    assert_untracked(&host, scope);
    let ctx: &dyn ResolverContext = &host;
    let db = host.project_type_store().prepared_target_db();
    let key = prepared_target_key(scope, scope);

    let _ = db.get_or_compute(&key, ctx, || {
        Some((
            Some((Arc::<str>::from(scope), Arc::<str>::from("stale"))),
            planted_self_root(scope),
        ))
    });

    let mut cold_ran = false;
    let warm = db
        .get_or_compute(&key, ctx, || {
            cold_ran = true;
            Some((
                Some((Arc::<str>::from(scope), Arc::<str>::from("recomputed"))),
                empty_fact_signature(),
            ))
        })
        .expect("warm path produces a value");

    assert!(
        cold_ran,
        "PreparedTargetDb MUST NOT serve a warm entry whose self-root names an \
         untracked keyed canonical",
    );
    assert_eq!(
        warm.map(|(_, n)| n.as_ref().to_string()),
        Some("recomputed".to_string()),
        "the rejected entry must not bubble its stale resolved target",
    );
}

/// `PreparedTargetDb`'s producer roots the **declaring** canonical as
/// a second self-root, distinct from the active scope. A content edit
/// to the declaring file invalidates the entry even when the active
/// scope is unchanged.
///
/// Discriminating property: the entry is keyed on `(active_scope,
/// decl_canonical)` with `decl_canonical != active_scope`. The entry
/// is cold-published with the EXACT signature the production producer
/// records — [`engine_fact_signature_for_prepared_target`], the named
/// helper `resolve_prepared_surface_target` calls. That helper roots
/// the declaring canonical as a second self-root. The declaring file
/// is then edited through the skip-own-drain hook (so the
/// own-canonical drain does NOT remove the entry), shifting its whole
/// hash. A producer helper that rooted only the active scope would
/// leave the entry valid (the active-scope self-root still matches)
/// and the warm read would serve stale; with the declaring canonical
/// rooted, the warm read misses and recomputes. Reverting the helper
/// to root only the active scope flips this test.
#[test]
fn prepared_target_db_declaring_canonical_edit_rejects_warm_entry() {
    use crate::resolver_core::component_meta_query_engine::engine_fact_signature_for_prepared_target;

    let host = VerterHost::new_standalone(HostConfig::default());
    let active_scope = "/self_root_qdb/ptgt_scope.ts";
    let decl_canonical = "/self_root_qdb/ptgt_decl.ts";
    upsert(
        &host,
        active_scope,
        "import { Probe } from './ptgt_decl';\nexport type ReExport = Probe;\n",
    );
    upsert(
        &host,
        decl_canonical,
        "export interface Probe { a: number; }\n",
    );
    host.ensure_indexed_ready(active_scope)
        .expect("scope indexed");
    host.ensure_indexed_ready(decl_canonical)
        .expect("decl indexed");

    let ctx: &dyn ResolverContext = &host;
    let db = host.project_type_store().prepared_target_db();
    let key = prepared_target_key(active_scope, decl_canonical);
    // The declaring canonical must be tracked at prime time so its
    // self-root validates and the entry is admitted — otherwise the
    // edit below is not the discriminator.
    {
        let view = ctx.resolver_store_view();
        assert!(
            StoreView::tracks_file(&view, decl_canonical),
            "fixture invariant: the declaring canonical is loaded and tracked",
        );
    }

    // Observe BOTH keyed canonicals' content versions at cold-publish
    // time, exactly as the production producer does — the
    // provenance-pure signature builder roots each self-root on its
    // observed hash.
    let observed_scope_hash = observed_whole_hash(ctx, active_scope);
    let observed_decl_hash = observed_whole_hash(ctx, decl_canonical);

    // Cold-publish with the EXACT production producer signature — the
    // named helper `resolve_prepared_surface_target` calls.
    let scope_owned = active_scope.to_string();
    let decl_owned = decl_canonical.to_string();
    let primed = db
        .get_or_compute(&key, ctx, || {
            let sig = engine_fact_signature_for_prepared_target(
                ctx,
                scope_owned.as_str(),
                "Probe",
                observed_scope_hash,
                decl_owned.as_str(),
                "Probe",
                observed_decl_hash,
            )
            .expect("provenance-pure signature builds — both observed artifacts present");
            Some((
                Some((Arc::<str>::from(decl_canonical), Arc::<str>::from("stale"))),
                sig,
            ))
        })
        .expect("cold publish succeeds — both keyed canonicals tracked");
    assert_eq!(
        primed.map(|(_, n)| n.as_ref().to_string()),
        Some("stale".to_string()),
        "fixture invariant: cold publish stores the primed target",
    );

    // Edit ONLY the declaring file through the skip-own-drain hook so
    // the own-canonical drain does not remove the cache entry.
    upsert_skip_drain(
        &host,
        decl_canonical,
        "export interface Probe { a: string; b: number; }\n",
    );
    host.ensure_indexed_ready(decl_canonical)
        .expect("decl re-indexed");

    let ctx2: &dyn ResolverContext = &host;
    let mut cold_ran = false;
    let warm = db
        .get_or_compute(&key, ctx2, || {
            cold_ran = true;
            Some((
                Some((
                    Arc::<str>::from(decl_canonical),
                    Arc::<str>::from("recomputed"),
                )),
                empty_fact_signature(),
            ))
        })
        .expect("warm path produces a value");

    assert!(
        cold_ran,
        "PreparedTargetDb warm read MUST reject the entry after a content edit to its \
         DECLARING canonical — the producer helper roots the declaring canonical as a \
         second self-root. A helper that rooted only the active scope would leave the \
         entry valid and serve stale.",
    );
    assert_eq!(
        warm.map(|(_, n)| n.as_ref().to_string()),
        Some("recomputed".to_string()),
        "the rejected warm entry must not bubble its stale resolved target",
    );
}

// ---------------------------------------------------------------------------
// Item 7 — PreparedSurfaceDb.
// ---------------------------------------------------------------------------

fn prepared_surface_key(canonical: &str) -> PreparedSurfaceCacheKey {
    PreparedSurfaceCacheKey {
        canonical_id: Arc::from(canonical),
        symbol_name: Arc::from("Probe"),
        substitutions: PreparedSubstitutionKey::Empty,
    }
}

/// `PreparedSurfaceDb` validates the keyed canonical's self-root
/// strictly (warm-hit `validate` AND post-compute revalidation). The
/// prepared surface encodes body-sensitive structure, so strict
/// self-root validation is the correctness floor.
///
/// Discriminating property: the prime attempt's payload is `Empty`,
/// the recompute produces `Unsupported`. A lazy validator admits the
/// `Empty` entry and the second `get_or_compute` returns it stale; the
/// strict validator rejects admission, so the recompute runs and
/// `Unsupported` surfaces.
#[test]
fn prepared_surface_db_untracked_self_root_rejects_warm_entry() {
    use crate::component_meta_caches::PreparedSurfacePayload;

    let host = host_with_unrelated_file();
    let c = "/self_root_qdb/psurf_never_loaded.ts";
    assert_untracked(&host, c);
    let ctx: &dyn ResolverContext = &host;
    let db = host.project_type_store().prepared_surface_db();
    let key = prepared_surface_key(c);

    let _ = db.get_or_compute(&key, ctx, || {
        Some((PreparedSurfacePayload::Empty, planted_self_root(c)))
    });

    let mut cold_ran = false;
    let warm = db
        .get_or_compute(&key, ctx, || {
            cold_ran = true;
            Some((PreparedSurfacePayload::Unsupported, empty_fact_signature()))
        })
        .expect("warm path produces a value");

    assert!(
        cold_ran,
        "PreparedSurfaceDb MUST NOT serve a warm entry whose self-root names an \
         untracked keyed canonical",
    );
    assert!(
        matches!(warm, PreparedSurfacePayload::Unsupported),
        "the rejected entry must not bubble its stale `Empty` payload",
    );
}

// ---------------------------------------------------------------------------
// Item 8 — PreparedMemberDb.
// ---------------------------------------------------------------------------

fn prepared_member_key(canonical: &str) -> PreparedMemberCacheKey {
    PreparedMemberCacheKey {
        canonical_id: Arc::from(canonical),
        symbol_name: Arc::from("Probe"),
        member_name: Arc::from("field"),
        kind: PreparedMemberCacheKind::Requested,
        substitutions: PreparedSubstitutionKey::Empty,
    }
}

fn projected_member(marker: &str) -> ProjectedMember {
    ProjectedMember {
        name: "field".to_string(),
        ty: TypeExpr::Unknown {
            raw: marker.to_string(),
        },
        optional: false,
        readonly: false,
        is_method: false,
    }
}

/// `PreparedMemberDb` validates the keyed canonical's self-root
/// strictly.
///
/// Discriminating property: the prime attempt's projected member
/// carries the marker `"stale"`; the recompute carries `"recomputed"`.
/// A lazy validator admits the stale member and the second
/// `get_or_compute` returns it; the strict validator rejects
/// admission.
#[test]
fn prepared_member_db_untracked_self_root_rejects_warm_entry() {
    let host = host_with_unrelated_file();
    let c = "/self_root_qdb/pmem_never_loaded.ts";
    assert_untracked(&host, c);
    let ctx: &dyn ResolverContext = &host;
    let db = host.project_type_store().prepared_member_db();
    let key = prepared_member_key(c);

    let _ = db.get_or_compute(&key, ctx, || {
        Some((Some(projected_member("stale")), planted_self_root(c)))
    });

    let mut cold_ran = false;
    let warm = db
        .get_or_compute(&key, ctx, || {
            cold_ran = true;
            Some((Some(projected_member("recomputed")), empty_fact_signature()))
        })
        .expect("warm path produces a value");

    assert!(
        cold_ran,
        "PreparedMemberDb MUST NOT serve a warm entry whose self-root names an \
         untracked keyed canonical",
    );
    assert!(
        matches!(
            warm.as_deref().map(|m| &m.ty),
            Some(TypeExpr::Unknown { raw }) if raw == "recomputed"
        ),
        "the rejected entry must not bubble its stale projected member",
    );
}

// ---------------------------------------------------------------------------
// Item 9 — RoutedExprSurfaceDb.
// ---------------------------------------------------------------------------

fn routed_expr_key(scope: &str) -> RoutedExprSurfaceCacheKey {
    RoutedExprSurfaceCacheKey {
        scope_canonical_id: Arc::from(scope),
        root_symbol: Arc::from("Probe"),
        route: RouteDemand::Whole,
    }
}

/// `RoutedExprSurfaceDb` validates the keyed scope canonical's
/// self-root strictly.
///
/// Discriminating property: the prime attempt's expression carries the
/// marker `"stale"`; the recompute carries `"recomputed"`. A lazy
/// validator admits the stale expression and the second
/// `get_or_compute` returns it; the strict validator rejects
/// admission.
#[test]
fn routed_expr_surface_db_untracked_self_root_rejects_warm_entry() {
    let host = host_with_unrelated_file();
    let c = "/self_root_qdb/routed_never_loaded.ts";
    assert_untracked(&host, c);
    let ctx: &dyn ResolverContext = &host;
    let db = host.project_type_store().routed_expr_surface_db();
    let key = routed_expr_key(c);

    let _ = db.get_or_compute(&key, ctx, || {
        Some((
            TypeExpr::Unknown {
                raw: "stale".to_string(),
            },
            planted_self_root(c),
        ))
    });

    let mut cold_ran = false;
    let warm = db
        .get_or_compute(&key, ctx, || {
            cold_ran = true;
            Some((
                TypeExpr::Unknown {
                    raw: "recomputed".to_string(),
                },
                empty_fact_signature(),
            ))
        })
        .expect("warm path produces a value");

    assert!(
        cold_ran,
        "RoutedExprSurfaceDb MUST NOT serve a warm entry whose self-root names an \
         untracked keyed canonical",
    );
    assert!(
        matches!(warm.as_ref(), TypeExpr::Unknown { raw } if raw == "recomputed"),
        "the rejected entry must not bubble its stale routed expression",
    );
}

// ---------------------------------------------------------------------------
// Item 10 — MaterializeMemoDb.
// ---------------------------------------------------------------------------

fn materialized(
    marker: &str,
    dep_signature: crate::semantic_query::DepSignature,
) -> MaterializedTypeExpr {
    MaterializedTypeExpr {
        node_id: None,
        type_expr: TypeExpr::Unknown {
            raw: marker.to_string(),
        },
        dep_signature,
    }
}

fn empty_dep_signature() -> crate::semantic_query::DepSignature {
    Arc::from(Vec::<(Arc<str>, crate::semantic_query::DepVersion)>::new())
}

/// Observe `canonical`'s `ShallowFileState::whole_hash` — the
/// content-version observation the provenance-pure query-cache
/// producers thread into their fact-signature builders. Tests call
/// this at cold-publish time to mirror the production producers, which
/// observe the keyed canonical's content version once at the value
/// source and thread it in.
fn observed_whole_hash(ctx: &dyn ResolverContext, canonical: &str) -> [u8; 16] {
    ctx.shallow_file_state(canonical)
        .unwrap_or_else(|| {
            panic!(
                "fixture invariant: {canonical} must have current shallow state so its \
                 observed whole hash is available"
            )
        })
        .whole_hash
}

/// Observe `scope`'s `SyntacticExportSet` parse fact pinned to a
/// specific content hash — the provenance-pure observation the
/// materialize-memo signature builder consumes. Tests call this with
/// the hash the keyed scope was loaded at to mirror the production
/// write-through, which observes the scope content version once and
/// threads it in.
fn observed_scope_export_set(
    ctx: &dyn ResolverContext,
    scope: &str,
    observed_whole_hash: [u8; 16],
) -> crate::resolver_core::ParseFactRef {
    crate::fact_signature_helpers::parse_fact_ref_for_observed_current_content(
        ctx,
        scope,
        observed_whole_hash,
        verter_semantic::facts::FactKey::SyntacticExportSet,
        verter_semantic::facts::FactLane::Semantic,
    )
    .unwrap_or_else(|| {
        panic!(
            "fixture invariant: scope {scope} must have a content-addressed artifact \
             at its observed whole hash so its SyntacticExportSet parse fact is recoverable",
        )
    })
}

/// Establish the single tear-free
/// [`MaterializeScopeObservation`] for `scope` through the production
/// [`ResolverContext::observe_materialize_scope`] path — the one
/// observation the materialize-memo publish site threads into
/// [`engine_fact_signature_for_materialize_memo`]. Tests call this so
/// the keyed-scope `whole_hash` AND the keyed-scope parse fact descend
/// from the SAME `IndexedReady`, matching the production write-through.
fn observe_scope(ctx: &dyn ResolverContext, scope: &str) -> MaterializeScopeObservation {
    ctx.observe_materialize_scope(scope).unwrap_or_else(|| {
        panic!(
            "fixture invariant: scope {scope} must have a tear-free materialize-scope \
             observation (a live scheduler DerivedRawState or a current artifact)",
        )
    })
}

/// `MaterializeMemoDb` validates the keyed scope canonical's self-root
/// strictly.
///
/// Discriminating property: the prime attempt's materialized
/// expression carries the marker `"stale"`; the recompute carries
/// `"recomputed"`. A lazy validator admits the stale expression and
/// the second `get_or_compute` returns it; the strict validator
/// rejects admission.
#[test]
fn materialize_memo_db_untracked_self_root_rejects_warm_entry() {
    use crate::semantic_query::ProjectionMode;

    let host = host_with_unrelated_file();
    let c = "/self_root_qdb/memo_never_loaded.ts";
    assert_untracked(&host, c);
    let ctx: &dyn ResolverContext = &host;
    let db = host.project_type_store().materialize_memo_db();
    let probe_expr = Arc::new(TypeExpr::Ref {
        name: Arc::from("Probe"),
        type_arguments: Arc::from(Vec::<TypeExpr>::new()),
    });
    let key = (
        Arc::<str>::from(c),
        Arc::clone(&probe_expr),
        ProjectionMode::Expanded,
    );

    let _ = db.get_or_compute(&key, ctx, || {
        Some((
            materialized("stale", empty_dep_signature()),
            planted_self_root(c),
        ))
    });

    let mut cold_ran = false;
    let warm = db
        .get_or_compute(&key, ctx, || {
            cold_ran = true;
            Some((
                materialized("recomputed", empty_dep_signature()),
                empty_fact_signature(),
            ))
        })
        .expect("warm path produces a value");

    assert!(
        cold_ran,
        "MaterializeMemoDb MUST NOT serve a warm entry whose self-root names an \
         untracked keyed canonical",
    );
    assert!(
        matches!(&warm.type_expr, TypeExpr::Unknown { raw } if raw == "recomputed"),
        "the rejected entry must not bubble its stale materialized expression",
    );
}

/// `MaterializeMemoDb`'s producer records a dependency `FileWholeHash`
/// for every canonical the materialization walk observed. A content
/// edit to an observed dependency invalidates the memo even though the
/// keyed scope canonical is unchanged.
///
/// Discriminating property: the entry is keyed on `scope` but its
/// `MaterializedTypeExpr.dep_signature` lists an observed dependency
/// `dep` (`dep != scope`). The entry is cold-published with the EXACT
/// signature the production producer records —
/// [`engine_fact_signature_for_materialize_memo`], the named helper
/// the materialize write-through calls — which merges every observed
/// canonical from the dep signature as a dependency `FileWholeHash`.
/// The dependency file is then edited through the skip-own-drain hook,
/// shifting `dep`'s whole hash. A producer helper that recorded only
/// the scope self-root would leave the entry valid and serve stale;
/// with the observed-dep fact recorded, the warm read misses and
/// recomputes. Reverting the helper to drop the observed-dep merge
/// flips this test.
///
/// On the skip-own-drain hook: `MaterializeMemoDb::invalidate_canonical`
/// matches only entries whose keyed *scope* equals the upserted
/// canonical, so a normal `upsert(dep)` would NOT drain this entry —
/// the entry is keyed on `scope`, not `dep`. The skip-drain hook is
/// used here for consistency with the sibling skip-drain canaries and
/// to keep the test isolated from any own-canonical drain side-effect;
/// the entry's survival across the dependency edit does not depend on
/// it. (The hook would matter for an edit to the keyed *scope*, whose
/// own-canonical drain does match the entry.)
#[test]
fn materialize_memo_db_observed_dependency_edit_rejects_warm_entry() {
    use crate::resolver_core::component_meta_query_engine::engine_fact_signature_for_materialize_memo;
    use crate::semantic_query::{DepVersion, ProjectionMode};

    let host = VerterHost::new_standalone(HostConfig::default());
    let scope = "/self_root_qdb/memo_scope.ts";
    let dep = "/self_root_qdb/memo_dep.ts";
    upsert(&host, scope, "export type Probe = number;\n");
    upsert(&host, dep, "export interface Helper { a: number; }\n");
    let scope_indexed = host.ensure_indexed_ready(scope).expect("scope indexed");
    let dep_indexed = host.ensure_indexed_ready(dep).expect("dep indexed");
    assert_ne!(
        scope_indexed.whole_hash, dep_indexed.whole_hash,
        "fixture invariant: scope and dependency have distinct whole hashes",
    );

    let ctx: &dyn ResolverContext = &host;
    let db = host.project_type_store().materialize_memo_db();
    let probe_expr = Arc::new(TypeExpr::Ref {
        name: Arc::from("Probe"),
        type_arguments: Arc::from(Vec::<TypeExpr>::new()),
    });
    let key = (
        Arc::<str>::from(scope),
        Arc::clone(&probe_expr),
        ProjectionMode::Expanded,
    );

    // The materialized value observed `dep` during materialization —
    // recorded on its `dep_signature`. The producer helper merges that
    // into the fact signature.
    let dep_sig: crate::semantic_query::DepSignature = Arc::from(vec![(
        Arc::<str>::from(dep),
        DepVersion::WholeHash(dep_indexed.whole_hash),
    )]);
    // The materialize write-through establishes ONE tear-free scope
    // observation and threads it into the provenance-pure signature
    // builder; the keyed-scope whole hash and the keyed-scope parse
    // fact both descend from that single observation.
    let observed_scope = observe_scope(ctx, scope);
    assert_eq!(
        observed_scope.whole_hash(),
        scope_indexed.whole_hash,
        "fixture invariant: the scope observation pins the scope's current IndexedReady",
    );
    let primed_dep_sig = Arc::clone(&dep_sig);
    let primed = db
        .get_or_compute(&key, ctx, move || {
            // `dep` is recorded as `DepVersion::WholeHash`, so the
            // signature builder returns `Some` and the entry is
            // admitted.
            let export_set = observed_scope.syntactic_export_set.clone()?;
            let sig = engine_fact_signature_for_materialize_memo(
                &observed_scope,
                export_set,
                &primed_dep_sig,
            )?;
            Some((materialized("stale", Arc::clone(&primed_dep_sig)), sig))
        })
        .expect("cold publish succeeds");
    assert!(
        matches!(&primed.type_expr, TypeExpr::Unknown { raw } if raw == "stale"),
        "fixture invariant: cold publish stores the primed materialized expression",
    );

    // Edit ONLY the observed dependency through the skip-own-drain
    // hook. (`MaterializeMemoDb::invalidate_canonical` matches only the
    // keyed scope, so a normal `upsert(dep)` would not drain this
    // scope-keyed entry anyway — the hook keeps the test isolated from
    // any own-canonical drain side-effect; see the docstring.)
    upsert_skip_drain(
        &host,
        dep,
        "export interface Helper { a: string; b: number; }\n",
    );
    host.ensure_indexed_ready(dep).expect("dep re-indexed");

    let ctx2: &dyn ResolverContext = &host;
    let mut cold_ran = false;
    let warm = db
        .get_or_compute(&key, ctx2, || {
            cold_ran = true;
            Some((
                materialized("recomputed", empty_dep_signature()),
                empty_fact_signature(),
            ))
        })
        .expect("warm path produces a value");

    assert!(
        cold_ran,
        "MaterializeMemoDb warm read MUST reject the entry after a content edit to a \
         canonical the materialization walk observed — the producer helper merges \
         every observed canonical into the fact signature as a dependency \
         FileWholeHash. A helper that recorded only the scope self-root would leave \
         the entry valid and serve stale.",
    );
    assert!(
        matches!(&warm.type_expr, TypeExpr::Unknown { raw } if raw == "recomputed"),
        "the rejected warm entry must not bubble its stale materialized expression",
    );
}

// ---------------------------------------------------------------------------
// End-to-end self-root skip-drain canaries.
//
// The `*_untracked_self_root_rejects_warm_entry` tests above prime each
// query-identity entry with a *synthetic* one-fact signature
// ([`planted_self_root`]) on a never-loaded canonical. They discriminate
// the strict-vs-lazy untracked-`FileWholeHash` validator behavior, but
// they bypass the producer helpers entirely — neutering the producer's
// self-root prepend does not flip them.
//
// The canaries below close that gap. Each one:
//
//  1. Loads a real `.ts` keyed canonical and `ensure_indexed_ready`s it
//     so it is TRACKED by the live store view. The keyed canonical
//     declares the cache's keyed type `Probe` PLUS an unrelated
//     sibling declaration `Sibling`.
//  2. Cold-publishes a query-identity entry whose signature is built by
//     the EXACT production producer helper the cache's real producer
//     calls (`engine_fact_signature_for_exported_type` /
//     `_for_canonical_member` / `_for_prepared_target` /
//     `_for_materialize_memo`).
//  3. Edits the keyed canonical through
//     [`crate::VerterHost::upsert_skipping_own_canonical_drain_for_tests`]
//     — so the upserted canonical's own-canonical drain does NOT remove
//     the entry — with an **unrelated-sibling-body edit**: a member of
//     the `Sibling` declaration changes type while `Probe` and the
//     file's export name set are left untouched.
//  4. Issues the warm read and asserts it MISSED (the cold closure ran)
//     and the recomputed value surfaced.
//
// Why an unrelated-sibling-body edit is the discriminator: the producer
// helpers emit a self-root `FileWholeHash` for the keyed canonical PLUS
// path-precise parse facts ABOUT THE KEYED TYPE. R14 makes those parse
// facts deliberately ignore an edit that the keyed type's declaration
// graph does not reach: `Export(Probe)` / `LocalDecl(Probe)` /
// `MemberShape(Probe)` / `Member(Probe, field)` / `MemberPresence(Probe,
// field)` all fingerprint `Probe` (or one keyed member of it), and
// `SyntacticExportSet` fingerprints the file's export NAME set. An edit
// to an *unrelated* `Sibling` declaration's member body shifts NONE of
// those — `Probe`'s body and reference-shape edges are unchanged, and
// `Sibling`'s export name is unchanged. ONLY the keyed canonical's
// whole-file hash shifts. So the warm read rejects the stale entry
// *iff* the producer recorded the self-root `FileWholeHash`. Reverting
// the `self_root_fact` prepend in the central fact-signature helpers
// makes every canary below FAIL — the stale entry validates
// (its path-precise parse facts are all unchanged) and is served warm,
// so the cold closure never runs. Each canary's docstring restates
// this property; the discrimination was verified by neutering
// `self_root_fact` and observing every canary flip RED.
//
// Scope note: these canaries drive the production producer + warm
// validator of each of the nine query-identity caches end-to-end. A
// canary that instead drove the full `get_component_meta` cold
// recompute over a skip-drain dependency edit would surface staleness
// in `FileArtifactStore` (a content-addressed cache, NOT one of the
// nine query-identity caches) before the nine caches are reached — see
// the feedback log's `[debt]` entry on `FileArtifactStore`
// content-pinning under the skip-drain hook. The comprehensive
// per-failing-class `get_component_meta`-level skip-drain canary
// closure is owned by the dedicated canary-closure work that follows;
// these nine producer-level canaries prove the self-version-root
// wiring end-to-end for its own scope.

/// The keyed canonical's source: the cache's keyed type `Probe` plus an
/// unrelated `Sibling` declaration whose body the canary edits.
fn keyed_source_with_sibling(sibling_member_ty: &str) -> String {
    format!(
        "export interface Probe {{ a: number; b: string; }}\n\
         export interface Sibling {{ x: {sibling_member_ty}; }}\n"
    )
}

/// Load `canonical` with `keyed_source_with_sibling("number")` and
/// `ensure_indexed_ready` it so the live store view tracks it; assert
/// the tracked invariant.
fn load_tracked_keyed(host: &VerterHost, canonical: &str) {
    upsert(host, canonical, &keyed_source_with_sibling("number"));
    assert!(
        host.ensure_indexed_ready(canonical).is_some(),
        "IndexedReady must materialise for {canonical}",
    );
    let ctx: &dyn ResolverContext = host;
    let view = ctx.resolver_store_view();
    assert!(
        StoreView::tracks_file(&view, canonical),
        "fixture invariant: {canonical} must be TRACKED so the unrelated-sibling-body \
         edit shifts only the self-root FileWholeHash, isolating the self-root as the \
         sole discriminating fact",
    );
}

/// Edit `canonical`'s unrelated `Sibling` declaration body through the
/// skip-drain hook (so the own-canonical drain does NOT remove the
/// entry) and re-`ensure_indexed_ready` it. `Probe` and the file's
/// export name set are untouched — only the whole-file hash shifts.
fn skip_drain_sibling_edit(host: &VerterHost, canonical: &str) {
    upsert_skip_drain(host, canonical, &keyed_source_with_sibling("string"));
    assert!(
        host.ensure_indexed_ready(canonical).is_some(),
        "IndexedReady must re-materialise for {canonical} after the sibling edit",
    );
}

/// `DeclarationLookupDb` — producer-level self-root canary.
///
/// Discriminating property: the entry is cold-published with the EXACT
/// production producer signature — [`engine_fact_signature_for_exported_type`],
/// the helper `resolve_type_declaration` calls. The keyed canonical is
/// then edited through the skip-drain hook with an unrelated-sibling
/// body edit. The producer signature's parse facts (`Export(Probe)`,
/// `LocalDecl(Probe)`, `MemberShape(Probe)`) all fingerprint `Probe`
/// (R14) and do NOT shift when an unrelated `Sibling` declaration is
/// edited — only the self-root `FileWholeHash` does. The warm read
/// therefore misses iff the producer recorded the self-root. Reverting
/// the `self_root_fact` prepend leaves the entry valid and serves it
/// stale (verified: neutering `self_root_fact` flips this canary RED).
#[test]
fn declaration_lookup_db_self_root_sibling_edit_rejects_warm_entry() {
    use crate::resolver_core::component_meta_query_engine::engine_fact_signature_for_exported_type;

    let host = VerterHost::new_standalone(HostConfig::default());
    let c = "/self_root_e2e/decl.ts";
    load_tracked_keyed(&host, c);
    let ctx: &dyn ResolverContext = &host;
    let db = host.project_type_store().declaration_db();
    let key = (Arc::<str>::from(c), Arc::<str>::from("Probe"));

    // Observe the keyed canonical's content version at cold-publish
    // time, exactly as the production producer does.
    let observed_keyed_hash = observed_whole_hash(ctx, c);
    let owned = c.to_string();
    let primed = db
        .get_or_compute(&key, ctx, || {
            let sig = engine_fact_signature_for_exported_type(
                ctx,
                owned.as_str(),
                "Probe",
                observed_keyed_hash,
            )
            .expect("provenance-pure signature builds — observed artifact present");
            Some((decl("stale", c), sig))
        })
        .expect("cold publish succeeds — keyed canonical tracked");
    assert_eq!(
        primed.text.as_deref(),
        Some("stale"),
        "fixture invariant: cold publish stores the primed declaration",
    );

    skip_drain_sibling_edit(&host, c);

    let ctx2: &dyn ResolverContext = &host;
    let mut cold_ran = false;
    let warm = db
        .get_or_compute(&key, ctx2, || {
            cold_ran = true;
            Some((decl("recomputed", c), empty_fact_signature()))
        })
        .expect("warm path produces a value");

    assert!(
        cold_ran,
        "DeclarationLookupDb warm read MUST reject the entry after an unrelated-sibling \
         edit to its keyed canonical — only the producer's self-root FileWholeHash \
         catches an edit the keyed type's declaration graph does not reach (the \
         Export/LocalDecl/MemberShape parse facts all fingerprint Probe). Reverting the \
         self_root_fact prepend leaves the entry valid and serves stale.",
    );
    assert_eq!(
        warm.text.as_deref(),
        Some("recomputed"),
        "the rejected warm entry must not bubble its stale declaration",
    );
}

/// `ImportedRegistryDb` — producer-level self-root canary. Identical
/// discrimination shape to the `DeclarationLookupDb` canary: the
/// production producer is [`engine_fact_signature_for_exported_type`]
/// (called by `resolve_imported_registry_symbol`) and an
/// unrelated-sibling edit shifts only the self-root `FileWholeHash`.
/// Verified: neutering `self_root_fact` flips this canary RED.
#[test]
fn imported_registry_db_self_root_sibling_edit_rejects_warm_entry() {
    use crate::resolver_core::component_meta_query_engine::engine_fact_signature_for_exported_type;

    let host = VerterHost::new_standalone(HostConfig::default());
    let c = "/self_root_e2e/imported.ts";
    load_tracked_keyed(&host, c);
    let ctx: &dyn ResolverContext = &host;
    let db = host.project_type_store().imported_registry_db();
    let key = (Arc::<str>::from(c), Arc::<str>::from("Probe"));

    // Observe the keyed canonical's content version at cold-publish
    // time, exactly as the production producer does.
    let observed_keyed_hash = observed_whole_hash(ctx, c);
    let owned = c.to_string();
    let _ = db
        .get_or_compute_admit(&key, ctx, || {
            let sig = engine_fact_signature_for_exported_type(
                ctx,
                owned.as_str(),
                "Probe",
                observed_keyed_hash,
            )
            .expect("provenance-pure signature builds — observed artifact present");
            crate::cooperative_admission::ComputeAdmission::Cacheable(
                crate::component_meta_caches::ImportedRegistryEntry {
                    value: Some(Arc::new(imported_symbol(c, "stale"))),
                    fact_dep_signature: sig,
                },
            )
        })
        .expect("cold publish succeeds");

    skip_drain_sibling_edit(&host, c);

    let ctx2: &dyn ResolverContext = &host;
    let mut cold_ran = false;
    let warm = db
        .get_or_compute_admit(&key, ctx2, || {
            cold_ran = true;
            crate::cooperative_admission::ComputeAdmission::Cacheable(
                crate::component_meta_caches::ImportedRegistryEntry {
                    value: Some(Arc::new(imported_symbol(c, "recomputed"))),
                    fact_dep_signature: empty_fact_signature(),
                },
            )
        })
        .expect("warm path produces a value");

    assert!(
        cold_ran,
        "ImportedRegistryDb warm read MUST reject the entry after an unrelated-sibling \
         edit to its keyed canonical — only the producer's self-root FileWholeHash \
         catches it. Reverting the self_root_fact prepend serves stale.",
    );
    assert_eq!(
        warm.map(|s| s.exported_name.clone()),
        Some("recomputed".to_string()),
        "the rejected warm entry must not bubble its stale resolved symbol",
    );
}

/// `ResolvabilityDb` — producer-level self-root canary. The production
/// producer is [`engine_fact_signature_for_exported_type`] (called by
/// `can_resolve_registry_symbol`); an unrelated-sibling edit shifts
/// only the self-root `FileWholeHash`. The primed value is `false`; the
/// recompute produces `true` — a directly observable boolean flip.
/// Verified: neutering `self_root_fact` flips this canary RED.
#[test]
fn resolvability_db_self_root_sibling_edit_rejects_warm_entry() {
    use crate::resolver_core::component_meta_query_engine::engine_fact_signature_for_exported_type;

    let host = VerterHost::new_standalone(HostConfig::default());
    let c = "/self_root_e2e/resolvable.ts";
    load_tracked_keyed(&host, c);
    let ctx: &dyn ResolverContext = &host;
    let db = host.project_type_store().resolvable_db();
    let key = (Arc::<str>::from(c), Arc::<str>::from("Probe"));

    // Observe the keyed canonical's content version at cold-publish
    // time, exactly as the production producer does.
    let observed_keyed_hash = observed_whole_hash(ctx, c);
    let owned = c.to_string();
    let _ = db
        .get_or_compute(&key, ctx, || {
            let sig = engine_fact_signature_for_exported_type(
                ctx,
                owned.as_str(),
                "Probe",
                observed_keyed_hash,
            )
            .expect("provenance-pure signature builds — observed artifact present");
            Some((false, sig))
        })
        .expect("cold publish succeeds");

    skip_drain_sibling_edit(&host, c);

    let ctx2: &dyn ResolverContext = &host;
    let mut cold_ran = false;
    let warm = db
        .get_or_compute(&key, ctx2, || {
            cold_ran = true;
            Some((true, empty_fact_signature()))
        })
        .expect("warm path produces a value");

    assert!(
        cold_ran,
        "ResolvabilityDb warm read MUST reject the entry after an unrelated-sibling \
         edit to its keyed canonical — only the producer's self-root FileWholeHash \
         catches it. Reverting the self_root_fact prepend serves stale.",
    );
    assert!(
        warm,
        "the rejected warm entry must not bubble its stale `false` — the recomputed \
         `true` must surface",
    );
}

/// `OwnerCollectionDb` — producer-level self-root canary. The
/// production producer is [`engine_fact_signature_for_exported_type`]
/// (called by `owner_collection_expr`); this cache is body-bearing
/// (stores a `TypeExpr`), so the self-root `FileWholeHash` is the
/// correctness floor. An unrelated-sibling edit shifts only the
/// self-root. Verified: neutering `self_root_fact` flips this canary
/// RED.
#[test]
fn owner_collection_db_self_root_sibling_edit_rejects_warm_entry() {
    use crate::resolver_core::component_meta_query_engine::engine_fact_signature_for_exported_type;

    let host = VerterHost::new_standalone(HostConfig::default());
    let c = "/self_root_e2e/owner.ts";
    load_tracked_keyed(&host, c);
    let ctx: &dyn ResolverContext = &host;
    let db = host.project_type_store().owner_collection_db();
    let key = (Arc::<str>::from(c), Arc::<str>::from("Probe"));

    // Observe the keyed canonical's content version at cold-publish
    // time, exactly as the production producer does.
    let observed_keyed_hash = observed_whole_hash(ctx, c);
    let owned = c.to_string();
    let _ = db
        .get_or_compute(&key, ctx, || {
            let sig = engine_fact_signature_for_exported_type(
                ctx,
                owned.as_str(),
                "Probe",
                observed_keyed_hash,
            )
            .expect("provenance-pure signature builds — observed artifact present");
            Some((
                Some(TypeExpr::Unknown {
                    raw: "stale".to_string(),
                }),
                sig,
            ))
        })
        .expect("cold publish succeeds");

    skip_drain_sibling_edit(&host, c);

    let ctx2: &dyn ResolverContext = &host;
    let mut cold_ran = false;
    let warm = db
        .get_or_compute(&key, ctx2, || {
            cold_ran = true;
            Some((
                Some(TypeExpr::Unknown {
                    raw: "recomputed".to_string(),
                }),
                empty_fact_signature(),
            ))
        })
        .expect("warm path produces a value");

    assert!(
        cold_ran,
        "OwnerCollectionDb warm read MUST reject the entry after an unrelated-sibling \
         edit to its keyed canonical — only the producer's self-root FileWholeHash \
         catches it. Reverting the self_root_fact prepend serves stale.",
    );
    assert!(
        matches!(warm.as_deref(), Some(TypeExpr::Unknown { raw }) if raw == "recomputed"),
        "the rejected warm entry must not bubble its stale body expression",
    );
}

/// `PreparedSurfaceDb` — producer-level self-root canary. The
/// production producer is [`engine_fact_signature_for_exported_type`]
/// (called by `publish_prepared_surface_to_host_db`); the prepared
/// surface encodes body-sensitive structure, so the self-root
/// `FileWholeHash` is the correctness floor. An unrelated-sibling edit
/// shifts only the self-root. Verified: neutering `self_root_fact`
/// flips this canary RED.
#[test]
fn prepared_surface_db_self_root_sibling_edit_rejects_warm_entry() {
    use crate::component_meta_caches::PreparedSurfacePayload;
    use crate::resolver_core::component_meta_query_engine::engine_fact_signature_for_exported_type;

    let host = VerterHost::new_standalone(HostConfig::default());
    let c = "/self_root_e2e/psurf.ts";
    load_tracked_keyed(&host, c);
    let ctx: &dyn ResolverContext = &host;
    let db = host.project_type_store().prepared_surface_db();
    let key = prepared_surface_key(c);

    // Observe the keyed canonical's content version at cold-publish
    // time, exactly as the production producer does.
    let observed_keyed_hash = observed_whole_hash(ctx, c);
    let owned = c.to_string();
    let _ = db
        .get_or_compute(&key, ctx, || {
            let sig = engine_fact_signature_for_exported_type(
                ctx,
                owned.as_str(),
                "Probe",
                observed_keyed_hash,
            )
            .expect("provenance-pure signature builds — observed artifact present");
            Some((PreparedSurfacePayload::Empty, sig))
        })
        .expect("cold publish succeeds");

    skip_drain_sibling_edit(&host, c);

    let ctx2: &dyn ResolverContext = &host;
    let mut cold_ran = false;
    let warm = db
        .get_or_compute(&key, ctx2, || {
            cold_ran = true;
            Some((PreparedSurfacePayload::Unsupported, empty_fact_signature()))
        })
        .expect("warm path produces a value");

    assert!(
        cold_ran,
        "PreparedSurfaceDb warm read MUST reject the entry after an unrelated-sibling \
         edit to its keyed canonical — only the producer's self-root FileWholeHash \
         catches it. Reverting the self_root_fact prepend serves stale.",
    );
    assert!(
        matches!(warm, PreparedSurfacePayload::Unsupported),
        "the rejected warm entry must not bubble its stale `Empty` payload",
    );
}

/// `RoutedExprSurfaceDb` — producer-level self-root canary. The
/// production producer is [`engine_fact_signature_for_exported_type`]
/// (called by `cache_routed_expr_surface_expr`); an unrelated-sibling
/// edit shifts only the self-root `FileWholeHash`. Verified: neutering
/// `self_root_fact` flips this canary RED.
#[test]
fn routed_expr_surface_db_self_root_sibling_edit_rejects_warm_entry() {
    use crate::resolver_core::component_meta_query_engine::engine_fact_signature_for_exported_type;

    let host = VerterHost::new_standalone(HostConfig::default());
    let c = "/self_root_e2e/routed.ts";
    load_tracked_keyed(&host, c);
    let ctx: &dyn ResolverContext = &host;
    let db = host.project_type_store().routed_expr_surface_db();
    let key = routed_expr_key(c);

    // Observe the keyed canonical's content version at cold-publish
    // time, exactly as the production producer does.
    let observed_keyed_hash = observed_whole_hash(ctx, c);
    let owned = c.to_string();
    let _ = db
        .get_or_compute(&key, ctx, || {
            let sig = engine_fact_signature_for_exported_type(
                ctx,
                owned.as_str(),
                "Probe",
                observed_keyed_hash,
            )
            .expect("provenance-pure signature builds — observed artifact present");
            Some((
                TypeExpr::Unknown {
                    raw: "stale".to_string(),
                },
                sig,
            ))
        })
        .expect("cold publish succeeds");

    skip_drain_sibling_edit(&host, c);

    let ctx2: &dyn ResolverContext = &host;
    let mut cold_ran = false;
    let warm = db
        .get_or_compute(&key, ctx2, || {
            cold_ran = true;
            Some((
                TypeExpr::Unknown {
                    raw: "recomputed".to_string(),
                },
                empty_fact_signature(),
            ))
        })
        .expect("warm path produces a value");

    assert!(
        cold_ran,
        "RoutedExprSurfaceDb warm read MUST reject the entry after an unrelated-sibling \
         edit to its keyed canonical — only the producer's self-root FileWholeHash \
         catches it. Reverting the self_root_fact prepend serves stale.",
    );
    assert!(
        matches!(warm.as_ref(), TypeExpr::Unknown { raw } if raw == "recomputed"),
        "the rejected warm entry must not bubble its stale routed expression",
    );
}

/// Count the named object-property members of a routed-expression
/// surface `TypeExpr`. Used to fingerprint which content version a
/// cached routed surface was projected from.
fn routed_surface_member_names(expr: &TypeExpr) -> Vec<String> {
    use verter_type_expr::ObjectMember;
    let mut names = Vec::new();
    if let TypeExpr::Object(object) = expr {
        for member in object.properties.iter() {
            match member {
                ObjectMember::Property(property) => names.push(property.name.clone()),
                ObjectMember::Method(method) => names.push(method.name.clone()),
                _ => {}
            }
        }
    }
    names.sort();
    names
}

/// `RoutedExprSurfaceDb` — producer-ordering canary for the
/// routed-expression observed-hash capture.
///
/// `project_routed_expr_surface_expr` projects the routed surface and
/// THEN calls `cache_routed_expr_surface_expr` to write it through. The
/// observed self-root content hash must be captured BEFORE the
/// projection, not inside the cache helper after it: capturing it after
/// the projection is a torn read — the projected value comes from one
/// content version, the self-root hash from a later one.
///
/// The fixture drives the race deterministically. A projection-seam
/// hook fires a skip-own-drain `upsert` of the keyed scope file
/// EXACTLY between the projection and the `cache_routed_expr_surface_expr`
/// write-through — the precise window the torn read opens. `Probe`'s
/// own member is renamed across the edit (`staleField` → `freshField`)
/// so the v1 routed surface and the v2 routed surface are
/// distinguishable, and the v1 value is provably stale once v2 lands.
///
/// Discrimination property — FAILS pre-fix, PASSES post-fix:
///
///  - Pre-fix (`cache_routed_expr_surface_expr` reads
///    `authoritative_current_content_hash` itself, after the projection
///    AND after the seam edit): the helper observes the POST-edit (v2)
///    hash, so the entry's self-root `FileWholeHash` is v2. Post-compute
///    revalidation against the (post-edit) v2 host validates that v2
///    self-root, so the entry is ADMITTED — carrying the stale v1
///    `projected_expr`. A warm `peek` against the v2 host then serves it.
///  - Post-fix (the caller captures the observed hash before the
///    projection and threads it in): the entry's self-root is the
///    PRE-edit (v1) hash; revalidation against the v2 host rejects it,
///    so the torn entry is NOT admitted. The warm `peek` misses and a
///    fresh request recomputes the v2 surface.
#[test]
fn routed_expr_surface_db_observed_hash_captured_before_projection_rejects_torn_entry() {
    use crate::resolver_core::component_meta_query_engine::{
        inject_routed_expr_projection_seam_edit_for_tests, ComponentMetaQueryEngine,
    };

    let probe_v1 = "export interface Probe { staleField: string; }\n";
    let probe_v2 = "export interface Probe { freshField: string; }\n";
    let c = "/routed_seam/probe.ts";

    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    upsert(&host, c, probe_v1);
    assert!(
        host.ensure_indexed_ready(c).is_some(),
        "fixture invariant: IndexedReady must materialise for {c}",
    );

    // The projection-seam hook: upsert v2 of the keyed scope file
    // through the skip-own-drain hook so the own-canonical drain does
    // NOT remove the entry the producer is about to publish — the
    // staleness must be caught by the producer's self-root hash alone.
    // The hook fires exactly between the routed-expression projection
    // and the `cache_routed_expr_surface_expr` write-through.
    let seam_host = Arc::clone(&host);
    let seam_path = c.to_string();
    let _seam = inject_routed_expr_projection_seam_edit_for_tests(move || {
        upsert_skip_drain(&seam_host, &seam_path, probe_v2);
        // Materialise v2's IndexedReady so the post-edit content-version
        // is a recoverable content-addressed artifact. The torn-read
        // producer roots the entry on this v2 hash and its signature
        // builds (and revalidates) against v2 — exactly the
        // "post-compute revalidation then passes" defect. Without a
        // recoverable v2 artifact the v2-rooted signature could not
        // build at all and the bug would be masked.
        assert!(
            seam_host.ensure_indexed_ready(&seam_path).is_some(),
            "seam-edit invariant: v2 IndexedReady must materialise",
        );
    });

    let route = RouteDemand::Whole;
    let projected = {
        let mut engine = ComponentMetaQueryEngine::new(&*host);
        engine
            .project_routed_expr_surface_expr(c, "Probe", &route)
            .expect("the routed-expression `Whole` surface of `Probe` projects")
    };
    // The producer projected v1 (the seam edit lands AFTER the
    // projection), so the returned value is the v1 surface regardless
    // of the fix — the discriminating fact is what the SHARED DB holds.
    assert_eq!(
        routed_surface_member_names(&projected),
        vec!["staleField".to_string()],
        "fixture invariant: the projection ran against v1 of `Probe`",
    );

    // The discriminating assertion: a warm `peek` of the shared
    // `RoutedExprSurfaceDb` against the POST-edit (v2) host. The key is
    // the same `(scope, root_symbol, route)` the producer wrote under.
    let ctx: &dyn ResolverContext = &*host;
    let db = host.project_type_store().routed_expr_surface_db();
    let arc_key = RoutedExprSurfaceCacheKey {
        scope_canonical_id: Arc::from(c),
        root_symbol: Arc::from("Probe"),
        route: route.clone(),
    };
    let warm = db.peek(&arc_key, ctx);
    assert!(
        warm.is_none(),
        "RoutedExprSurfaceDb MUST NOT hold a warm entry after the projection-seam edit: \
         the torn-read producer roots the entry's self-root on the POST-edit hash, so it \
         validates against the post-edit host and serves the stale v1 routed surface. \
         Capturing the observed hash before the projection roots it on the pre-edit hash, \
         so the torn entry is refused admission. warm = {:?}",
        warm.as_deref(),
    );

    // A fresh request (new engine — request-local scratch is per-engine)
    // recomputes the v2 surface.
    let mut fresh_engine = ComponentMetaQueryEngine::new(&*host);
    let fresh = fresh_engine
        .project_routed_expr_surface_expr(c, "Probe", &route)
        .expect("the routed-expression `Whole` surface of `Probe` re-projects against v2");
    assert_eq!(
        routed_surface_member_names(&fresh),
        vec!["freshField".to_string()],
        "the recomputed routed surface must reflect the v2 content (`freshField`), \
         not the stale v1 `staleField`",
    );
}

/// `PreparedTargetDb` — producer-level self-root canary. The production
/// producer is [`engine_fact_signature_for_prepared_target`] (called by
/// `resolve_prepared_surface_target`), which roots BOTH the active
/// scope and the declaring canonical via
/// `engine_fact_signature_for_exported_type`. Here the active scope and
/// the declaring canonical are the same file; an unrelated-sibling edit
/// shifts only the self-root `FileWholeHash`.
///
/// This canary complements
/// [`prepared_target_db_declaring_canonical_edit_rejects_warm_entry`]:
/// that test edits the declaring canonical with a member-*shape* edit
/// (adds a member to `Probe`) and so discriminates the producer's
/// declaring-canonical `MemberShape` parse fact; this one edits an
/// unrelated `Sibling` declaration and so discriminates the producer's
/// self-root `FileWholeHash`. Verified: neutering `self_root_fact`
/// flips this canary RED.
#[test]
fn prepared_target_db_self_root_sibling_edit_rejects_warm_entry() {
    use crate::resolver_core::component_meta_query_engine::engine_fact_signature_for_prepared_target;

    let host = VerterHost::new_standalone(HostConfig::default());
    let c = "/self_root_e2e/ptgt.ts";
    load_tracked_keyed(&host, c);
    let ctx: &dyn ResolverContext = &host;
    let db = host.project_type_store().prepared_target_db();
    let key = prepared_target_key(c, c);

    // Observe the keyed canonical's content version at cold-publish
    // time — here the active scope and the declaring canonical are the
    // same file, so one observation roots both self-roots.
    let observed_keyed_hash = observed_whole_hash(ctx, c);
    let owned = c.to_string();
    let _ = db
        .get_or_compute(&key, ctx, || {
            let sig = engine_fact_signature_for_prepared_target(
                ctx,
                owned.as_str(),
                "Probe",
                observed_keyed_hash,
                owned.as_str(),
                "Probe",
                observed_keyed_hash,
            )
            .expect("provenance-pure signature builds — observed artifact present");
            Some((Some((Arc::<str>::from(c), Arc::<str>::from("stale"))), sig))
        })
        .expect("cold publish succeeds");

    skip_drain_sibling_edit(&host, c);

    let ctx2: &dyn ResolverContext = &host;
    let mut cold_ran = false;
    let warm = db
        .get_or_compute(&key, ctx2, || {
            cold_ran = true;
            Some((
                Some((Arc::<str>::from(c), Arc::<str>::from("recomputed"))),
                empty_fact_signature(),
            ))
        })
        .expect("warm path produces a value");

    assert!(
        cold_ran,
        "PreparedTargetDb warm read MUST reject the entry after an unrelated-sibling \
         edit to its keyed canonical — only the producer's self-root FileWholeHash \
         catches it. Reverting the self_root_fact prepend serves stale.",
    );
    assert_eq!(
        warm.map(|(_, n)| n.as_ref().to_string()),
        Some("recomputed".to_string()),
        "the rejected warm entry must not bubble its stale resolved target",
    );
}

/// `MaterializeMemoDb` — producer-level self-root canary. The
/// production producer is [`engine_fact_signature_for_materialize_memo`]
/// (called by the materialize write-through). It is provenance-pure:
/// the publish site observes the scope content version once and threads
/// the observed whole hash plus the observed-version `SyntacticExportSet`
/// parse fact in; the builder roots the keyed scope's self-root
/// `FileWholeHash` on that observed hash. An unrelated-sibling body edit
/// to the scope canonical shifts only the self-root `FileWholeHash` —
/// the `SyntacticExportSet` parse fact fingerprints the export NAME set
/// (unchanged when an existing `Sibling`'s member body is edited).
///
/// This canary complements
/// [`materialize_memo_db_observed_dependency_edit_rejects_warm_entry`]:
/// that test edits an observed *dependency* and so discriminates the
/// producer's observed-dep merge; this one edits the keyed *scope*
/// canonical and so discriminates the producer's self-root
/// `FileWholeHash`. The cold publish roots the entry on the scope hash
/// observed at cold-publish time; the sibling edit shifts that hash, so
/// the strict warm-read self-root validation misses. A builder that
/// emitted no scope self-root `FileWholeHash` would leave the entry
/// valid and serve stale.
#[test]
fn materialize_memo_db_self_root_sibling_edit_rejects_warm_entry() {
    use crate::resolver_core::component_meta_query_engine::engine_fact_signature_for_materialize_memo;
    use crate::semantic_query::ProjectionMode;

    let host = VerterHost::new_standalone(HostConfig::default());
    let c = "/self_root_e2e/memo.ts";
    load_tracked_keyed(&host, c);
    let ctx: &dyn ResolverContext = &host;
    // The single tear-free scope observation taken at
    // materialisation/cold-publish time — `observe_materialize_scope`
    // pins the scope's current `IndexedReady`.
    let db = host.project_type_store().materialize_memo_db();
    let probe_expr = Arc::new(TypeExpr::Ref {
        name: Arc::from("Probe"),
        type_arguments: Arc::from(Vec::<TypeExpr>::new()),
    });
    let key = (
        Arc::<str>::from(c),
        Arc::clone(&probe_expr),
        ProjectionMode::Expanded,
    );

    let observed_scope = observe_scope(ctx, c);
    let _ = db
        .get_or_compute(&key, ctx, || {
            // No observed dependencies — the signature builder returns
            // `Some` (no `RouteGeneration` entry) and the discriminator
            // is the scope canonical's own self-root `FileWholeHash`,
            // rooted on the observation's content version.
            let export_set = observed_scope.syntactic_export_set.clone()?;
            let sig = engine_fact_signature_for_materialize_memo(
                &observed_scope,
                export_set,
                &empty_dep_signature(),
            )?;
            Some((materialized("stale", empty_dep_signature()), sig))
        })
        .expect("cold publish succeeds");

    skip_drain_sibling_edit(&host, c);

    let ctx2: &dyn ResolverContext = &host;
    let mut cold_ran = false;
    let warm = db
        .get_or_compute(&key, ctx2, || {
            cold_ran = true;
            Some((
                materialized("recomputed", empty_dep_signature()),
                empty_fact_signature(),
            ))
        })
        .expect("warm path produces a value");

    assert!(
        cold_ran,
        "MaterializeMemoDb warm read MUST reject the entry after an unrelated-sibling \
         edit to its keyed scope canonical — only the producer's self-root \
         FileWholeHash catches it (the SyntacticExportSet parse fact fingerprints the \
         export name set, unchanged). Reverting the self_root_fact prepend serves \
         stale.",
    );
    assert!(
        matches!(&warm.type_expr, TypeExpr::Unknown { raw } if raw == "recomputed"),
        "the rejected warm entry must not bubble its stale materialized expression",
    );
}

/// `PreparedMemberDb` — producer-level self-root canary. The production
/// producer is [`engine_fact_signature_for_canonical_member`] (called
/// by `publish_prepared_member_to_host_db`), which records the keyed
/// member's `MemberPresence` + `Member` parse facts PLUS the keyed
/// canonical's self-root `FileWholeHash`.
///
/// Discriminating property: the canary edits an unrelated `Sibling`
/// declaration's member body, NOT the keyed `Probe.field` member. The
/// keyed member's `MemberPresence(Probe, field)` and `Member(Probe,
/// field)` parse facts are path-precise (R28) and stay unchanged by an
/// edit `Probe.field`'s declaration graph does not reach — only the
/// keyed canonical's self-root `FileWholeHash` shifts. The warm read
/// therefore misses iff the producer recorded the self-root; reverting
/// the `self_root_fact` prepend leaves the entry valid and serves the
/// stale projected member. Verified: neutering `self_root_fact` flips
/// this canary RED.
#[test]
fn prepared_member_db_self_root_sibling_edit_rejects_warm_entry() {
    use crate::resolver_core::component_meta_query_engine::engine_fact_signature_for_canonical_member;

    let host = VerterHost::new_standalone(HostConfig::default());
    let c = "/self_root_e2e/pmem.ts";
    // `Probe` carries the keyed member `field`; `Sibling` is the
    // unrelated declaration the canary edits.
    upsert(
        &host,
        c,
        "export interface Probe { field: number; }\n\
         export interface Sibling { x: number; }\n",
    );
    assert!(
        host.ensure_indexed_ready(c).is_some(),
        "IndexedReady must materialise for {c}",
    );
    let ctx: &dyn ResolverContext = &host;
    {
        let view = ctx.resolver_store_view();
        assert!(
            StoreView::tracks_file(&view, c),
            "fixture invariant: {c} must be TRACKED",
        );
    }
    let db = host.project_type_store().prepared_member_db();
    let key = prepared_member_key(c);

    // Observe the keyed canonical's content version at cold-publish
    // time, exactly as the production producer does.
    let observed_keyed_hash = observed_whole_hash(ctx, c);
    let owned = c.to_string();
    let _ = db
        .get_or_compute(&key, ctx, || {
            let sig = engine_fact_signature_for_canonical_member(
                ctx,
                owned.as_str(),
                "Probe",
                "field",
                observed_keyed_hash,
            )
            .expect("provenance-pure signature builds — observed artifact present");
            Some((Some(projected_member("stale")), sig))
        })
        .expect("cold publish succeeds");

    // Unrelated-sibling body edit: `Sibling.x` changes type; the keyed
    // member `Probe.field` is untouched, so `MemberPresence(Probe,
    // field)` and `Member(Probe, field)` are unchanged — only the
    // self-root `FileWholeHash` shifts.
    upsert_skip_drain(
        &host,
        c,
        "export interface Probe { field: number; }\n\
         export interface Sibling { x: string; }\n",
    );
    assert!(
        host.ensure_indexed_ready(c).is_some(),
        "IndexedReady must re-materialise for {c}",
    );

    let ctx2: &dyn ResolverContext = &host;
    let mut cold_ran = false;
    let warm = db
        .get_or_compute(&key, ctx2, || {
            cold_ran = true;
            Some((Some(projected_member("recomputed")), empty_fact_signature()))
        })
        .expect("warm path produces a value");

    assert!(
        cold_ran,
        "PreparedMemberDb warm read MUST reject the entry after an unrelated-sibling \
         edit to its keyed canonical — the keyed member's MemberPresence/Member parse \
         facts are path-precise and do not shift on an edit Probe.field's declaration \
         graph does not reach, so only the producer's self-root FileWholeHash catches \
         it. Reverting the self_root_fact prepend serves the stale projected member.",
    );
    assert!(
        matches!(
            warm.as_deref().map(|m| &m.ty),
            Some(TypeExpr::Unknown { raw }) if raw == "recomputed"
        ),
        "the rejected warm entry must not bubble its stale projected member",
    );
}

/// `MaterializeMemoDb`'s producer REFUSES shared-memo admission for an
/// entry whose materialisation walk observed a dependency via
/// `DepVersion::RouteGeneration`.
///
/// Route generation is not a real production-validating fact: there is
/// no authoritative route-generation counter, no production emitter,
/// and `HostFenceValidator` treats `RouteGeneration` as always-valid.
/// Rooting a `RouteGeneration`-observed dependency with any fact would
/// be unsound — no fact can detect a content edit to that file — so
/// [`engine_fact_signature_for_materialize_memo`] returns `None` and
/// the publish site declines to insert the entry. The freshly-computed
/// `MaterializedTypeExpr` is still returned to the caller; only the
/// shared-cache admission is refused.
///
/// This test was previously
/// `materialize_memo_db_non_whole_hash_observed_dependency_edit_rejects_warm_entry`
/// and asserted the OLD behavior — that the producer rooted a
/// `RouteGeneration`-observed canonical by re-reading its
/// current-content `FileWholeHash`. That rooting was the edit/publish
/// race: a current-content hash re-read at signature-build time roots
/// the entry by the POST-edit hash of a dependency edited between
/// materialisation and publish, so the stale value warm-validates. The
/// strict-validation behavior legitimately changed — a
/// `RouteGeneration` dependency now refuses admission outright — so the
/// test is rewritten to assert the new, correct behavior.
///
/// Discriminating property: the `materialized_dep_signature` carries a
/// `(dep, DepVersion::RouteGeneration(_))` entry.
/// `engine_fact_signature_for_materialize_memo` MUST return `None`. A
/// producer that re-reads `dep`'s current-content hash (the old body)
/// returns `Some` and admits the entry — this test FAILS against that
/// body (`assert!(sig.is_none())` trips, and `db.live_count()` is `1`
/// not `0`). A producer that refuses `RouteGeneration` returns `None`,
/// admits nothing, and a follow-up request cold-recomputes — this test
/// PASSES.
#[test]
fn materialize_memo_db_route_generation_observed_dependency_refuses_admission() {
    use crate::resolver_core::component_meta_query_engine::engine_fact_signature_for_materialize_memo;
    use crate::semantic_query::{DepVersion, ProjectionMode};

    let host = VerterHost::new_standalone(HostConfig::default());
    let scope = "/self_root_e2e/memo_rg_scope.ts";
    let dep = "/self_root_e2e/memo_rg_dep.ts";
    upsert(&host, scope, "export type Probe = number;\n");
    upsert(&host, dep, "export interface Helper { a: number; }\n");
    assert!(
        host.ensure_indexed_ready(scope).is_some(),
        "scope IndexedReady materialises",
    );
    assert!(
        host.ensure_indexed_ready(dep).is_some(),
        "dep IndexedReady materialises",
    );

    let ctx: &dyn ResolverContext = &host;
    let db = host.project_type_store().materialize_memo_db();
    let probe_expr = Arc::new(TypeExpr::Ref {
        name: Arc::from("Probe"),
        type_arguments: Arc::from(Vec::<TypeExpr>::new()),
    });
    let key = (
        Arc::<str>::from(scope),
        Arc::clone(&probe_expr),
        ProjectionMode::Expanded,
    );

    // The materialisation walk observed `dep` via a `RouteGeneration`
    // dependency — route generation has no validating source.
    let dep_sig: crate::semantic_query::DepSignature = Arc::from(vec![(
        Arc::<str>::from(dep),
        DepVersion::RouteGeneration(1),
    )]);

    // The signature builder MUST refuse admission — `None`. The scope's
    // observed identity is supplied (the builder is provenance-pure),
    // so the refusal here is driven solely by the `RouteGeneration`
    // dependency, not by a missing scope observation. A producer that
    // rooted the `RouteGeneration` dependency by any fact returns
    // `Some(...)` and this assertion trips.
    let observed_scope = observe_scope(ctx, scope);
    let sig = engine_fact_signature_for_materialize_memo(
        &observed_scope,
        observed_scope
            .syntactic_export_set
            .clone()
            .expect("scope SyntacticExportSet parse fact recoverable"),
        &dep_sig,
    );
    assert!(
        sig.is_none(),
        "engine_fact_signature_for_materialize_memo MUST return None when an observed \
         dependency carries DepVersion::RouteGeneration — route generation has no \
         validating source, so the entry must not be admitted to the shared memo. A \
         producer that roots the RouteGeneration dependency by any fact returns \
         Some and admits the entry; that admission is the unsoundness this refusal \
         closes.",
    );

    // Drive the publish path exactly as the production write-through
    // does: the closure threads the `None` signature through `?`, so
    // `get_or_compute`'s compute returns `None` and nothing is
    // admitted.
    let primed_dep_sig = Arc::clone(&dep_sig);
    let cold_value = db.get_or_compute(&key, ctx, move || {
        let export_set = observed_scope.syntactic_export_set.clone()?;
        let fact_sig = engine_fact_signature_for_materialize_memo(
            &observed_scope,
            export_set,
            &primed_dep_sig,
        )?;
        Some((materialized("fresh", Arc::clone(&primed_dep_sig)), fact_sig))
    });
    // The publish path declined the entry — `get_or_compute` returns
    // `None` because its compute closure short-circuited.
    assert!(
        cold_value.is_none(),
        "the publish path threads the None signature through `?`, so get_or_compute \
         declines to insert and returns None",
    );
    assert_eq!(
        db.live_count(),
        0,
        "no MaterializeMemoDb entry may be admitted when the fact signature is refused \
         — the shared memo stays empty",
    );

    // A get_component_meta-style follow-up request still produces the
    // correct freshly-computed value — refusing shared-cache admission
    // does NOT change observable request output, it only forgoes the
    // memo. The cold closure runs because no entry was admitted.
    let ctx2: &dyn ResolverContext = &host;
    let mut cold_ran = false;
    let value = db
        .get_or_compute(&key, ctx2, || {
            cold_ran = true;
            Some((
                materialized("recomputed", empty_dep_signature()),
                empty_fact_signature(),
            ))
        })
        .expect("the follow-up request still computes and returns a value");
    assert!(
        cold_ran,
        "the follow-up request's cold closure MUST run — no entry was admitted, so there \
         is no warm hit to short-circuit it",
    );
    assert!(
        matches!(&value.type_expr, TypeExpr::Unknown { raw } if raw == "recomputed"),
        "the refused-admission path still returns the freshly-computed value to the \
         caller — user-visible request output is unaffected",
    );
}

/// `MaterializeMemoDb`'s producer roots a dependency the materialiser
/// observed as `DepVersion::ProjectGeneration` by a
/// [`crate::resolver_core::FactVersionRef::ProjectGeneration`] carrying
/// the OBSERVED generation — NOT by a `FileWholeHash`.
///
/// A `ProjectGeneration` dependency means the materialised value
/// observed the project-wide resolver/config/lib generation, not that
/// file's content. The correct root is therefore the observed
/// generation: a project-shape change (`tsconfig`, path-alias, SDK,
/// workspace-folder, project-graph) bumps the counter and rejects the
/// memo; a pure file-content edit does not bump it and so does not
/// over-invalidate.
///
/// Discriminating property — two halves:
///
/// 1. Signature shape. The produced signature MUST carry a
///    `FactVersionRef::ProjectGeneration { generation: g_observed }`
///    and MUST NOT carry any `FileWholeHash` for `dep`. The old body
///    rooted a non-`WholeHash` dependency by re-reading its
///    current-content `FileWholeHash`; this test FAILS against that
///    body (the negative `FileWholeHash`-for-`dep` assertion trips,
///    and no `ProjectGeneration` fact is found).
/// 2. Warm rejection on generation advance. A memo entry rooted on
///    observed generation `g` is REJECTED on warm read once the
///    project generation advances past `g`. The generation is advanced
///    with the bare `bump_project_generation()` (NOT
///    `bump_project_generation_and_evict()`), so the entry is NOT
///    evicted by the bump itself — the warm read misses purely because
///    the `ProjectGeneration` fact's observed generation no longer
///    equals the current one. A producer that emitted no
///    `ProjectGeneration` fact (or rooted by `FileWholeHash` instead)
///    would leave the entry valid and serve it stale.
#[test]
fn materialize_memo_db_project_generation_observed_dependency_roots_on_observed_generation() {
    use crate::resolver_core::component_meta_query_engine::engine_fact_signature_for_materialize_memo;
    use crate::semantic_query::{DepVersion, ProjectionMode};

    let host = VerterHost::new_standalone(HostConfig::default());
    let scope = "/self_root_e2e/memo_pg_scope.ts";
    let dep = "/self_root_e2e/memo_pg_dep.ts";
    upsert(&host, scope, "export type Probe = number;\n");
    upsert(&host, dep, "export interface Helper { a: number; }\n");
    assert!(
        host.ensure_indexed_ready(scope).is_some(),
        "scope IndexedReady materialises",
    );
    assert!(
        host.ensure_indexed_ready(dep).is_some(),
        "dep IndexedReady materialises",
    );

    let ctx: &dyn ResolverContext = &host;
    let db = host.project_type_store().materialize_memo_db();
    let probe_expr = Arc::new(TypeExpr::Ref {
        name: Arc::from("Probe"),
        type_arguments: Arc::from(Vec::<TypeExpr>::new()),
    });
    let key = (
        Arc::<str>::from(scope),
        Arc::clone(&probe_expr),
        ProjectionMode::Expanded,
    );

    // The materialiser observed `dep` against the project-wide
    // generation `g_observed` — the host's current project generation.
    let g_observed = host.project_type_store().project_generation();
    let dep_sig: crate::semantic_query::DepSignature = Arc::from(vec![(
        Arc::<str>::from(dep),
        DepVersion::ProjectGeneration(g_observed),
    )]);

    // Half 1 — signature shape.
    let observed_scope = observe_scope(ctx, scope);
    let sig = engine_fact_signature_for_materialize_memo(
        &observed_scope,
        observed_scope
            .syntactic_export_set
            .clone()
            .expect("scope SyntacticExportSet parse fact recoverable"),
        &dep_sig,
    )
    .expect("a ProjectGeneration dep signature is admissible (Some)");
    assert!(
        sig.iter().any(|f| matches!(
            f,
            FactVersionRef::ProjectGeneration { generation } if *generation == g_observed
        )),
        "the producer MUST root a ProjectGeneration-observed dependency by a \
         FactVersionRef::ProjectGeneration carrying the OBSERVED generation \
         ({g_observed})",
    );
    assert!(
        !sig.iter().any(|f| matches!(
            f,
            FactVersionRef::FileWholeHash { canonical_id, .. } if canonical_id == dep
        )),
        "the producer MUST NOT root a ProjectGeneration-observed dependency by a \
         FileWholeHash for {dep} — a ProjectGeneration dependency observed the \
         project-wide generation, not the file's content. Re-reading the current \
         content hash is the edit/publish-race defect.",
    );

    // Prime the memo with the production signature.
    let primed_dep_sig = Arc::clone(&dep_sig);
    let primed = db
        .get_or_compute(&key, ctx, move || {
            let export_set = observed_scope.syntactic_export_set.clone()?;
            let fact_sig = engine_fact_signature_for_materialize_memo(
                &observed_scope,
                export_set,
                &primed_dep_sig,
            )?;
            Some((materialized("stale", Arc::clone(&primed_dep_sig)), fact_sig))
        })
        .expect("cold publish succeeds");
    assert!(
        matches!(&primed.type_expr, TypeExpr::Unknown { raw } if raw == "stale"),
        "fixture invariant: cold publish stores the primed materialised expression",
    );
    assert_eq!(
        db.live_count(),
        1,
        "fixture invariant: the ProjectGeneration-rooted entry is admitted",
    );

    // Half 2 — advance the project generation WITHOUT evicting any
    // cache. The bare `bump_project_generation()` increments the
    // counter only; the entry stays in the DB, so the warm read must
    // miss purely because the ProjectGeneration fact's observed
    // generation no longer matches the current one.
    let g_after = host.project_type_store().bump_project_generation();
    assert!(
        g_after > g_observed,
        "fixture invariant: the project generation advanced past the observed value",
    );
    assert_eq!(
        db.live_count(),
        1,
        "fixture invariant: bump_project_generation does NOT evict the entry — the warm \
         read must reject it on the ProjectGeneration fact alone",
    );

    let ctx2: &dyn ResolverContext = &host;
    let mut cold_ran = false;
    let warm = db
        .get_or_compute(&key, ctx2, || {
            cold_ran = true;
            Some((
                materialized("recomputed", empty_dep_signature()),
                empty_fact_signature(),
            ))
        })
        .expect("warm path produces a value");
    assert!(
        cold_ran,
        "MaterializeMemoDb warm read MUST reject the entry once the project generation \
         advances past the observed generation — the ProjectGeneration fact's observed \
         value no longer equals the current generation. A producer that emitted no \
         ProjectGeneration fact (or rooted by FileWholeHash instead) would leave the \
         entry valid and serve stale.",
    );
    assert!(
        matches!(&warm.type_expr, TypeExpr::Unknown { raw } if raw == "recomputed"),
        "the rejected warm entry must not bubble its stale materialised expression",
    );
}

/// `MaterializeMemoDb`'s producer preserves the OBSERVED `WholeHash`
/// for a dependency the materialiser recorded as
/// [`crate::semantic_query::DepVersion::WholeHash`] — it must NOT
/// re-read the dependency's current content hash.
///
/// Discriminating property — the materialise → write-through race
/// window. The materialiser observes `dep` at content hash `H1` and
/// records `DepVersion::WholeHash(H1)` on the materialised value's
/// `dep_signature`. `dep` is then edited so its CURRENT content hash
/// becomes `H2` (`H2 != H1`). Only AFTER that edit is the
/// materialize-memo fact signature built — exactly the ordering of an
/// edit that lands between materialisation and the
/// [`engine_fact_signature_for_materialize_memo`] write-through.
///
/// The emitted `FileWholeHash` fact for `dep` MUST carry the observed
/// `H1`, not the current `H2`:
///
/// - A producer that re-reads `dep`'s current content hash emits `H2`.
///   The stale `MaterializedTypeExpr` would then be published rooted by
///   a fresh-looking current hash and would VALIDATE on every warm read
///   — the staleness is permanently masked.
/// - A producer that preserves the observed hash emits `H1`. A warm
///   read validates `H1` against `dep`'s current `H2`, mismatches, and
///   the memo correctly misses.
///
/// This test does not exercise warm-read admission (the sibling
/// `materialize_memo_db_observed_dependency_edit_rejects_warm_entry`
/// covers that, but only when the signature is built BEFORE the edit,
/// so it cannot see the race-window bug). It inspects the emitted fact
/// signature directly: it FAILS against a producer that re-reads the
/// current hash (asserts `H1`, gets `H2`) and PASSES against a producer
/// that preserves the observed hash.
#[test]
fn materialize_memo_db_observed_whole_hash_dependency_preserves_observed_hash() {
    use crate::resolver_core::component_meta_query_engine::engine_fact_signature_for_materialize_memo;
    use crate::semantic_query::DepVersion;

    let host = VerterHost::new_standalone(HostConfig::default());
    let scope = "/self_root_race/memo_scope.ts";
    let dep = "/self_root_race/memo_dep.ts";
    upsert(&host, scope, "export type Probe = number;\n");
    upsert(&host, dep, "export interface Helper { a: number; }\n");
    let scope_indexed = host.ensure_indexed_ready(scope).expect("scope indexed");
    let dep_indexed_h1 = host.ensure_indexed_ready(dep).expect("dep indexed at H1");
    let observed_hash_h1 = dep_indexed_h1.whole_hash;
    assert_ne!(
        scope_indexed.whole_hash, observed_hash_h1,
        "fixture invariant: scope and dependency have distinct whole hashes",
    );

    // The materialiser observed `dep` at content hash H1 and recorded a
    // `DepVersion::WholeHash(H1)` entry on the materialised value's
    // `dep_signature`.
    let dep_sig: crate::semantic_query::DepSignature = Arc::from(vec![(
        Arc::<str>::from(dep),
        DepVersion::WholeHash(observed_hash_h1),
    )]);

    // The dependency is edited AFTER materialisation but BEFORE the
    // memo signature is built — the race window. Its current content
    // hash shifts to H2. The skip-drain hook keeps the test isolated
    // from any own-canonical drain side-effect (`MaterializeMemoDb`
    // keys on the scope, not `dep`, so this is purely defensive).
    upsert_skip_drain(
        &host,
        dep,
        "export interface Helper { a: string; b: number; }\n",
    );
    let dep_indexed_h2 = host
        .ensure_indexed_ready(dep)
        .expect("dep re-indexed at H2");
    let current_hash_h2 = dep_indexed_h2.whole_hash;
    assert_ne!(
        observed_hash_h1, current_hash_h2,
        "fixture invariant: the dependency edit shifts its whole hash (H1 != H2)",
    );

    // Build the materialize-memo fact signature NOW — after the edit —
    // from the materialised value's `dep_signature` (which still
    // carries the observed H1). A producer that re-reads `dep`'s
    // current content emits H2; one that preserves the observed hash
    // emits H1.
    let ctx: &dyn ResolverContext = &host;
    // The scope canonical is untouched by the dependency edit, so its
    // tear-free observation (and the content-addressed artifact backing
    // its `SyntacticExportSet` parse fact) is still recoverable.
    let observed_scope = observe_scope(ctx, scope);
    assert_eq!(
        observed_scope.whole_hash(),
        scope_indexed.whole_hash,
        "fixture invariant: the scope observation is unchanged by the dependency edit",
    );
    // `dep` is recorded as `DepVersion::WholeHash`, so the signature
    // builder admits the entry (`Some`).
    let sig = engine_fact_signature_for_materialize_memo(
        &observed_scope,
        observed_scope
            .syntactic_export_set
            .clone()
            .expect("scope SyntacticExportSet parse fact recoverable"),
        &dep_sig,
    )
    .expect("a WholeHash-only dep signature must produce an admissible fact signature");

    let dep_fact_hash = sig.iter().find_map(|f| match f {
        FactVersionRef::FileWholeHash { canonical_id, hash } if canonical_id == dep => Some(*hash),
        _ => None,
    });
    let dep_fact_hash = dep_fact_hash.expect(
        "the materialize-memo signature MUST root the observed dependency by a \
         FileWholeHash fact",
    );

    assert_eq!(
        dep_fact_hash, observed_hash_h1,
        "MaterializeMemoDb's producer MUST preserve the OBSERVED WholeHash (H1) the \
         materialiser recorded for a DepVersion::WholeHash dependency — it must NOT \
         re-read the dependency's current content hash. Emitting the current hash (H2) \
         would publish the stale MaterializedTypeExpr rooted by a fresh-looking hash \
         that validates on every warm read, permanently masking an edit landing in the \
         materialise -> write-through race window.",
    );
    assert_ne!(
        dep_fact_hash, current_hash_h2,
        "the emitted dependency fact must NOT carry the dependency's post-edit current \
         content hash (H2) — re-reading the current hash is the race-window defect.",
    );
}

/// `engine_fact_signature_for_materialize_memo` roots the keyed scope's
/// self-root `FileWholeHash` on the caller-supplied OBSERVED hash — it
/// must never re-read the scope's current content hash.
///
/// Discrimination property: the builder is provenance-pure. It is
/// handed `observed_scope_whole_hash = H_observed` while the scope
/// canonical's CURRENT content hash is a distinct `H_current`
/// (`H_current != H_observed`). The emitted scope `FileWholeHash` MUST
/// carry `H_observed`. A builder that ignored the parameter and
/// re-read the scope's current content (the round-3 P1 defect: the
/// scope hash was read twice non-atomically — once for the value, once
/// for the signature) would emit `H_current`; the stale value would
/// then be published rooted by a fresh-looking current hash and
/// validate on every warm read, permanently masking an edit landing in
/// the materialise -> write-through race window.
///
/// RED proof (this fix changed the builder signature): with the body
/// reverted to re-read the scope's current content hash for the
/// self-root (the round-3 P1 defect), the emitted scope `FileWholeHash`
/// carries `H_current` and the `assert_eq!(.., H_observed)` trips. The
/// post-fix body emits the caller-supplied parameter and the assertion
/// holds.
///
/// The observed-version `SyntacticExportSet` parse fact is captured
/// BEFORE the scope edit — exactly as the production publish site does
/// (it observes the scope content version once, synchronously, at
/// materialisation time, then threads the observation in). A re-upsert
/// removes the prior content-hash artifact from the content-addressed
/// `FileArtifactStore`, so a parse fact observed AFTER the edit would
/// be unrecoverable; the production ordering observes it first.
#[test]
fn materialize_memo_db_scope_self_root_carries_observed_hash_not_current() {
    use crate::resolver_core::component_meta_query_engine::engine_fact_signature_for_materialize_memo;

    let host = VerterHost::new_standalone(HostConfig::default());
    let scope = "/self_root_race/memo_scope_observed.ts";
    // Load the scope at content version A — the hash the materialiser
    // observes when it lowers the value.
    upsert(&host, scope, "export type Probe = number;\n");
    let observed_hash = host
        .ensure_indexed_ready(scope)
        .expect("scope indexed at observed version")
        .whole_hash;

    let ctx: &dyn ResolverContext = &host;
    // Capture the ONE tear-free scope observation NOW — before any
    // edit — mirroring the production publish site, which calls
    // `observe_materialize_scope` once at materialisation time. The
    // observation carries BOTH the observed `whole_hash` AND the
    // observed-version `SyntacticExportSet` parse fact, from one
    // `IndexedReady` — they cannot disagree.
    let observed_scope = observe_scope(ctx, scope);
    assert_eq!(
        observed_scope.whole_hash(),
        observed_hash,
        "fixture invariant: the observation pins the scope's pre-edit content version",
    );

    // The scope is edited AFTER materialisation but BEFORE the memo
    // signature is built — the race window. Its CURRENT content hash
    // shifts. The skip-drain hook keeps the scope-keyed entry undrained
    // (irrelevant here — no entry is published — but keeps the fixture
    // isolated from the own-canonical drain).
    upsert_skip_drain(&host, scope, "export type Probe = string;\n");
    let current_hash = host
        .ensure_indexed_ready(scope)
        .expect("scope re-indexed at current version")
        .whole_hash;
    assert_ne!(
        observed_hash, current_hash,
        "fixture invariant: the scope edit shifts its whole hash (observed != current)",
    );

    // Build the signature NOW — after the edit — from the SINGLE
    // pre-edit observation captured above, exactly as the publish site
    // does after observing the scope content version once at
    // materialisation time. The observation's `whole_hash` and parse
    // fact are both pre-edit; a builder that re-read current content
    // would emit `current_hash`.
    let observed_export_set = observed_scope
        .syntactic_export_set
        .clone()
        .expect("the pre-edit observation carries the scope's SyntacticExportSet parse fact");
    let sig = engine_fact_signature_for_materialize_memo(
        &observed_scope,
        observed_export_set,
        &empty_dep_signature(),
    )
    .expect("a dep-free materialize-memo signature is admissible");

    let scope_fact_hash = sig
        .iter()
        .find_map(|f| match f {
            FactVersionRef::FileWholeHash { canonical_id, hash } if canonical_id == scope => {
                Some(*hash)
            }
            _ => None,
        })
        .expect("the materialize-memo signature MUST root the keyed scope by a FileWholeHash");

    assert_eq!(
        scope_fact_hash, observed_hash,
        "engine_fact_signature_for_materialize_memo MUST root the keyed scope's self-root \
         FileWholeHash on the caller-supplied OBSERVED hash — it must NOT re-read the \
         scope's current content. Emitting the current hash publishes the stale \
         MaterializedTypeExpr rooted by a fresh-looking hash that validates on every warm \
         read, masking an edit in the materialise -> write-through race window.",
    );
    assert_ne!(
        scope_fact_hash, current_hash,
        "the emitted scope self-root must NOT carry the scope's post-edit current content \
         hash — re-reading the current hash is the round-3 P1 publish-race defect.",
    );
}

/// End-to-end: a scope edit landing between the value's hash
/// observation and the `MaterializeMemoDb` write-through is caught by
/// the `revalidate_after_compute` hook, so the stale entry is NOT
/// admitted and a follow-up request cold-recomputes.
///
/// Discrimination property: this drives the publish-path
/// `get_or_compute` closure exactly as the production write-through
/// does, but with the scope edited in the race window — the value is
/// built against the observed pre-edit hash `H1`; the scope is then
/// upserted to `H2`; only THEN is the fact signature built (passing the
/// observed `H1`). The provenance-pure builder roots the entry's
/// self-root on `H1`; `revalidate_after_compute` validates that `H1`
/// self-root strictly against the scope's current `H2`, mismatches, and
/// the entry is NOT admitted — `get_or_compute` returns `None`. A
/// follow-up request therefore cold-recomputes and the fresh value
/// surfaces.
///
/// A builder that re-read the scope's current content at
/// signature-build time would root the self-root on `H2`;
/// `revalidate_after_compute` would then validate `H2` against the
/// current `H2`, the entry WOULD be admitted, and the follow-up request
/// would be a warm hit serving the stale value — `cold_ran` stays
/// `false`. This test FAILS against that body and PASSES against the
/// provenance-pure builder.
#[test]
fn materialize_memo_db_scope_edit_in_race_window_rejects_stale_entry_end_to_end() {
    use crate::resolver_core::component_meta_query_engine::engine_fact_signature_for_materialize_memo;
    use crate::semantic_query::ProjectionMode;

    let host = VerterHost::new_standalone(HostConfig::default());
    let scope = "/self_root_race/memo_scope_e2e.ts";
    upsert(&host, scope, "export type Probe = number;\n");
    // The materialiser observes the scope at content version H1 — the
    // hash baked into the value's NodeScopeId.
    let observed_hash_h1 = host
        .ensure_indexed_ready(scope)
        .expect("scope indexed at H1")
        .whole_hash;

    let ctx: &dyn ResolverContext = &host;
    let db = host.project_type_store().materialize_memo_db();
    let probe_expr = Arc::new(TypeExpr::Ref {
        name: Arc::from("Probe"),
        type_arguments: Arc::from(Vec::<TypeExpr>::new()),
    });
    let key = (
        Arc::<str>::from(scope),
        Arc::clone(&probe_expr),
        ProjectionMode::Expanded,
    );

    // The publish site calls `observe_materialize_scope` once,
    // synchronously, at materialisation time — BEFORE any racing edit.
    // Capture that single observation now to mirror the ordering (a
    // re-upsert removes the prior content-hash artifact from the
    // content-addressed store).
    let observed_scope_h1 = observe_scope(ctx, scope);
    assert_eq!(
        observed_scope_h1.whole_hash(),
        observed_hash_h1,
        "fixture invariant: the observation pins the scope's pre-edit H1 content version",
    );

    // The scope is edited AFTER the value's hash observation but BEFORE
    // the write-through builds the fact signature — the exact race
    // window the round-3 P1 describes.
    upsert_skip_drain(&host, scope, "export type Probe = string;\n");
    let current_hash_h2 = host
        .ensure_indexed_ready(scope)
        .expect("scope re-indexed at H2")
        .whole_hash;
    assert_ne!(
        observed_hash_h1, current_hash_h2,
        "fixture invariant: the race-window scope edit shifts its whole hash (H1 != H2)",
    );

    // Drive the publish closure exactly as the production write-through
    // does: the value was materialised against the OBSERVED H1, so the
    // signature is built from the single pre-edit observation (the
    // publish site threads it in). The provenance-pure builder roots
    // the scope self-root on H1 — the observation's `whole_hash`.
    let cold_value = db.get_or_compute(&key, ctx, move || {
        let export_set = observed_scope_h1.syntactic_export_set.clone()?;
        let fact_sig = engine_fact_signature_for_materialize_memo(
            &observed_scope_h1,
            export_set,
            &empty_dep_signature(),
        )?;
        Some((materialized("stale", empty_dep_signature()), fact_sig))
    });
    assert!(
        cold_value.is_none(),
        "the publish path MUST NOT admit a MaterializeMemoDb entry whose value was \
         materialised against the pre-edit scope content — revalidate_after_compute \
         validates the H1 self-root against the scope's current H2, mismatches, and \
         declines the insert. A builder that re-read the current hash would root the \
         self-root on H2, revalidate H2-vs-H2 successfully, and admit the stale entry.",
    );
    assert_eq!(
        db.live_count(),
        0,
        "no entry may be admitted — the stale value materialised against pre-edit content \
         must not warm the shared memo",
    );

    // A follow-up request cold-recomputes because no entry was admitted.
    let ctx2: &dyn ResolverContext = &host;
    let mut cold_ran = false;
    let value = db
        .get_or_compute(&key, ctx2, || {
            cold_ran = true;
            Some((
                materialized("recomputed", empty_dep_signature()),
                empty_fact_signature(),
            ))
        })
        .expect("the follow-up request still computes a value");
    assert!(
        cold_ran,
        "the follow-up request's cold closure MUST run — the race-window staleness was \
         rejected, so there is no warm hit to short-circuit it. A builder that re-read \
         the current hash would have admitted the stale entry and this would be a warm \
         hit instead.",
    );
    assert!(
        matches!(&value.type_expr, TypeExpr::Unknown { raw } if raw == "recomputed"),
        "the rejected race-window entry must not bubble its stale materialised expression",
    );
}

/// `engine_fact_signature_for_materialize_memo` REFUSES shared-memo
/// admission when an observed dependency names the keyed scope itself
/// with a `WholeHash` that disagrees with the caller-supplied observed
/// scope hash — a torn / mixed observation of the scope.
///
/// Discrimination property: the `materialized_dep_signature` carries a
/// `(scope, DepVersion::WholeHash(h_disagree))` entry where
/// `h_disagree != observed_scope_whole_hash`. The builder's
/// scope-collapse branch MUST detect the disagreement and return
/// `None`. A builder that unconditionally `continue`s on a
/// scope-named dependency (skipping the equality check) would return
/// `Some` and admit an entry whose self-root and dep observation
/// disagree on the scope's content version. This test FAILS against
/// that body (`assert!(sig.is_none())` trips) and PASSES against the
/// post-fix body that performs the equality check.
#[test]
fn materialize_memo_db_mixed_scope_observation_refuses_admission() {
    use crate::resolver_core::component_meta_query_engine::engine_fact_signature_for_materialize_memo;
    use crate::semantic_query::DepVersion;

    let host = VerterHost::new_standalone(HostConfig::default());
    let scope = "/self_root_race/memo_mixed_scope.ts";
    upsert(&host, scope, "export type Probe = number;\n");
    assert!(host.ensure_indexed_ready(scope).is_some(), "scope indexed",);

    let ctx: &dyn ResolverContext = &host;
    // The single tear-free scope observation the publish site threads
    // into the builder.
    let observed_scope = observe_scope(ctx, scope);
    let observed_scope_hash = observed_scope.whole_hash();

    // A disagreeing hash for the SAME scope canonical — distinct from
    // the observed scope hash, simulating a sub-dispatch that recorded
    // the scope at a different content version than the one the
    // publish site observed.
    let mut h_disagree = observed_scope_hash;
    h_disagree[0] ^= 0xFF;
    assert_ne!(
        h_disagree, observed_scope_hash,
        "fixture invariant: the disagreeing scope hash differs from the observed one",
    );

    // The materialisation walk recorded the scope itself with a
    // WholeHash that disagrees with the observed scope hash.
    let dep_sig: crate::semantic_query::DepSignature = Arc::from(vec![(
        Arc::<str>::from(scope),
        DepVersion::WholeHash(h_disagree),
    )]);

    let sig = engine_fact_signature_for_materialize_memo(
        &observed_scope,
        observed_scope
            .syntactic_export_set
            .clone()
            .expect("scope SyntacticExportSet parse fact recoverable"),
        &dep_sig,
    );
    assert!(
        sig.is_none(),
        "engine_fact_signature_for_materialize_memo MUST return None when an observed \
         dependency names the keyed scope with a WholeHash disagreeing with the \
         observation's whole hash — a torn read of the scope's content \
         version. A builder that unconditionally skips a scope-named dependency admits \
         an entry whose self-root and dep observation disagree.",
    );
}

/// `engine_fact_signature_for_materialize_memo` REFUSES shared-memo
/// admission when the caller-supplied `SyntacticExportSet` parse fact
/// describes a canonical OTHER than the keyed scope.
///
/// Discrimination property: the builder is handed an
/// `observed_scope_syntactic_export_set` whose `canonical_id` is a
/// different file than `scope_canonical_id`. The builder MUST reject
/// the mismatched observation and return `None` — a `Parse` fact for
/// the wrong file would mis-root the entry. A builder that pushed the
/// supplied parse fact without checking its canonical would return
/// `Some` and emit a self-root signature describing two different
/// files. This test FAILS against that body and PASSES against the
/// post-fix body that performs the canonical-equality guard.
#[test]
fn materialize_memo_db_scope_export_set_canonical_mismatch_refuses_admission() {
    use crate::resolver_core::component_meta_query_engine::engine_fact_signature_for_materialize_memo;

    let host = VerterHost::new_standalone(HostConfig::default());
    let scope = "/self_root_race/memo_canon_scope.ts";
    let other = "/self_root_race/memo_canon_other.ts";
    upsert(&host, scope, "export type Probe = number;\n");
    upsert(&host, other, "export type Other = number;\n");
    assert!(host.ensure_indexed_ready(scope).is_some(), "scope indexed",);
    let observed_other_hash = host
        .ensure_indexed_ready(other)
        .expect("other indexed")
        .whole_hash;

    let ctx: &dyn ResolverContext = &host;
    // The observation describes the keyed `scope`, but the parse fact
    // passed alongside it describes `other` — a mismatched observation
    // that must refuse admission.
    let observed_scope = observe_scope(ctx, scope);
    let mismatched_export_set = observed_scope_export_set(ctx, other, observed_other_hash);
    assert_eq!(
        mismatched_export_set.canonical_id, other,
        "fixture invariant: the supplied parse fact describes `other`, not the keyed scope",
    );

    let sig = engine_fact_signature_for_materialize_memo(
        &observed_scope,
        mismatched_export_set,
        &empty_dep_signature(),
    );
    assert!(
        sig.is_none(),
        "engine_fact_signature_for_materialize_memo MUST return None when the supplied \
         SyntacticExportSet parse fact describes a canonical other than the keyed scope — \
         a Parse fact for the wrong file mis-roots the entry. A builder that pushes the \
         supplied parse fact without a canonical-equality guard emits a self-root \
         signature describing two different files.",
    );
}

/// The materialize-memo publish site lowers the scope's `TypeExpr`
/// under the scope's REAL content version even for a scope reachable
/// only through the artifact path — a genuinely artifact-only file
/// (foreign source or test-seeded `IndexedReady`) that was NEVER
/// registered with the scheduler as a live `DerivedRawState`.
///
/// Root cause this test guards: the publish site
/// (`meta_resolve/materialize/field_types.rs`) needs ONE tear-free
/// observation of the scope's content identity. The
/// `NodeScopeId::File { whole_hash }` the materialiser LOWERS against
/// AND the `MaterializeMemoDb` signature self-root MUST agree —
/// `observe_materialize_scope` collapses both onto one
/// `Arc<IndexedReady>`. The scheduler-pinned authority
/// (`current_content_pinned_indexed`) answers only for files with a
/// live scheduler `DerivedRawState`. A genuinely artifact-only file
/// has a perfectly valid `IndexedReady` in `FileArtifactStore` but no
/// scheduler source, so `observe_materialize_scope` MUST also consult
/// the **artifact-current authority** (`artifact_current_indexed`).
///
/// ## Fixture — a genuinely artifact-only scope
///
/// A real `IndexedReady` (with a resolvable `Probe` interface) is
/// materialised for a helper file, then published into
/// `FileArtifactStore` under a SEPARATE canonical that the scheduler
/// never tracked. That canonical therefore has:
///
/// - no `DerivedRawState` entry → `authoritative_current_content_hash`
///   returns `None`, so `current_content_pinned_indexed` returns
///   `None`;
/// - a current `FileArtifactStore` artifact (not evicted — there is no
///   `DerivedRawState` entry to carry an `evicted` flag) →
///   `artifact_current_indexed` returns `Some`.
///
/// The fixture asserts exactly this before materialising.
///
/// ## Discrimination property
///
/// A 0-arg `Ref` to a same-file type lowers (in `Navigate` mode) to a
/// `SemanticNodeData::DeclRef` carrier whose `NodeScopeId` scope is the
/// lowering scope verbatim — the `NodeScopeId::File` the publish site
/// builds from `observe_materialize_scope(scope).whole_hash()`. The
/// test materialises `Probe` and scans the semantic graph for that
/// `DeclRef` node:
///
/// - WITHOUT the artifact-current authority (`observe_materialize_scope`
///   scheduler-only): the genuinely artifact-only scope yields a
///   `None` observation, the publish site's
///   `.expect("materialize scope must have a real indexed scope
///   identity")` PANICS — the test fails.
/// - WITH the artifact-current authority: `observe_materialize_scope`
///   returns `Some` rooted on the artifact's real `whole_hash`, the
///   `DeclRef` node's `NodeScopeId` scope `whole_hash` is that real
///   non-zero hash, and both assertions hold.
///
/// Reverting `observe_materialize_scope` to drop the
/// `artifact_current_indexed` fallback flips this test RED (publish-site
/// panic). The `assert_eq!(.., real_hash)` independently pins that the
/// lowering scope hash is the observation's hash — not a fabricated
/// `[0; 16]` (the publish site contains no `unwrap_or_default`).
#[test]
fn materialize_memo_db_artifact_only_scope_lowers_under_shallow_whole_hash() {
    use crate::resolver_core::ComponentMetaQueryEngine;
    use crate::semantic_query::{NodeScopeId, ProjectionMode, SemanticNodeId};

    let host = VerterHost::new_standalone(HostConfig::default());

    // Build a real `IndexedReady` (with a resolvable `Probe`
    // interface) by loading a helper file through the production path.
    let helper = "/self_root_race/memo_artifact_only_helper.ts";
    upsert(&host, helper, "export interface Probe { a: number; }\n");
    let helper_indexed = host
        .ensure_indexed_ready(helper)
        .expect("helper IndexedReady materialises");
    let real_hash = helper_indexed.whole_hash;
    assert_ne!(
        real_hash, [0u8; 16],
        "fixture invariant: the artifact's real content hash is non-zero",
    );

    // Publish that `IndexedReady` into `FileArtifactStore` under a
    // SEPARATE canonical the scheduler never tracked — a genuinely
    // artifact-only scope (codex P2's "foreign source or test-seeded
    // file with no scheduler DerivedRawState").
    let scope = "/self_root_race/memo_artifact_only_scope.ts";
    host.project_type_store()
        .indexed()
        .insert(Arc::from(scope), Arc::clone(&helper_indexed));

    let ctx: &dyn ResolverContext = &host;
    // Fixture invariant: the scheduler-pinned authority cannot answer
    // for this scope (no `DerivedRawState`), but the artifact-current
    // authority can (a current, non-evicted `FileArtifactStore`
    // artifact).
    assert_eq!(
        ctx.authoritative_current_content_hash(scope),
        None,
        "fixture invariant: a genuinely artifact-only scope has no scheduler \
         DerivedRawState, so `authoritative_current_content_hash` returns None",
    );
    let observation = ctx.observe_materialize_scope(scope).expect(
        "fixture invariant: `observe_materialize_scope` MUST return Some for a \
         genuinely artifact-only scope via the artifact-current authority — a \
         current FileArtifactStore artifact with no DerivedRawState eviction",
    );
    assert_eq!(
        observation.whole_hash(),
        real_hash,
        "fixture invariant: the observation pins the artifact's real whole hash",
    );

    // Materialise a 0-arg `Ref` to the same-file `Probe` interface in
    // `Navigate` mode. In `Navigate` mode a 0-arg local `Ref` lowers to
    // a `DeclRef` carrier interned with the lowering `NodeScopeId`
    // verbatim — the `NodeScopeId::File` the publish site builds from
    // the single `observe_materialize_scope` observation.
    let probe_expr = TypeExpr::Ref {
        name: Arc::from("Probe"),
        type_arguments: Arc::from(Vec::<TypeExpr>::new()),
    };
    let mut engine = ComponentMetaQueryEngine::new(&host);
    let _materialized =
        crate::meta_resolve::materialize::materialize_component_meta_type_expr_until_stable_full(
            &probe_expr,
            scope,
            ProjectionMode::Navigate,
            &mut engine,
        );

    // Scan the semantic graph for the lowered `DeclRef` node naming
    // `Probe` in the scope file. Its `NodeScopeId` scope is the
    // lowering scope the publish site constructed.
    let graph = host.project_type_store().semantic_graph();
    let decl_ref_scope_hash = (0..graph.node_count() as u64).find_map(|i| {
        let id = SemanticNodeId(i);
        let data = graph.node_data(id)?;
        let names_probe = matches!(
            &*data,
            crate::semantic_query::SemanticNodeData::DeclRef { identity }
                if identity.canonical_id.as_ref() == scope
                    && identity.decl_name.as_ref() == "Probe"
        );
        if !names_probe {
            return None;
        }
        match graph.node_scope(id)? {
            NodeScopeId::File { whole_hash, .. } => Some(whole_hash),
            NodeScopeId::Global => None,
        }
    });
    let decl_ref_scope_hash = decl_ref_scope_hash.expect(
        "the Navigate-mode materialisation of a same-file `Ref` MUST lower it to a \
         `DeclRef` carrier interned with a `NodeScopeId::File` scope",
    );

    assert_ne!(
        decl_ref_scope_hash, [0u8; 16],
        "the materialise-memo publish site MUST NOT lower the scope's TypeExpr under an \
         all-zero `NodeScopeId::File` whole hash. The lowering scope hash is sourced from \
         the single `observe_materialize_scope` observation — which answers for a \
         genuinely artifact-only scope via the artifact-current authority. The publish \
         site contains no `unwrap_or_default`: a missing observation would panic the \
         `expect`, never fabricate `[0; 16]`.",
    );
    assert_eq!(
        decl_ref_scope_hash, real_hash,
        "the lowered `DeclRef` node's `NodeScopeId::File` whole hash MUST equal the \
         scope's real content version — `observe_materialize_scope(scope).whole_hash()`, \
         the version of the file actually being materialised. The lowering scope and the \
         signature self-root both descend from this one observation, so they cannot \
         disagree.",
    );
}

// ---------------------------------------------------------------------------
// Provenance-pure publish-race discriminators — one per live query cache.
//
// Each test below drives the precise publish-race the provenance-pure
// signature builders close: a query-cache producer that re-reads the keyed
// canonical's CURRENT content hash at signature-build time roots a value on
// post-edit content when an `upsert` lands between the value-compute and the
// signature-build. `revalidate_after_compute` validates fresh-vs-fresh and
// cannot catch it.
//
// Discrimination shape (deterministic, no artifact-survival dependency — the
// upsert path drains every prior content version of a canonical from
// `FileArtifactStore`, so a stale observed hash has no content-addressed
// artifact):
//
//  1. Load the keyed canonical at content version `H1`; observe `H1`.
//  2. Anchor non-vacuity: the producer signature builder called with
//     `observed_hash = H1` while current == `H1` returns `Some` rooted on
//     `H1`.
//  3. Edit the keyed canonical through the skip-own-drain hook so current
//     becomes a different `H2` (the prior `H1` artifact is drained).
//  4. The producer signature builder called with the STALE `observed_hash =
//     H1` MUST return `None` — the observed version's parse-fact registry is
//     unrecoverable, so shared-cache admission is refused. A pre-fix builder
//     that re-reads current content resolves `H2`'s registry and returns
//     `Some` rooted on `H2` — this test FAILS against that body.
//  5. The same builder called with the CURRENT `observed_hash = H2` returns
//     `Some` rooted on `H2` — proving the step-4 `None` is specifically the
//     stale-observation refusal, not a systematically broken builder.
//
// Reverting any of the three helpers to a current-content re-read flips every
// test below RED at step 4.

/// True iff `signature` carries a `FileWholeHash` self-root for
/// `canonical` whose hash equals `expected`.
fn signature_roots_whole_hash(
    signature: &[FactVersionRef],
    canonical: &str,
    expected: [u8; 16],
) -> bool {
    signature.iter().any(|fact| {
        matches!(
            fact,
            FactVersionRef::FileWholeHash { canonical_id, hash }
                if canonical_id == canonical && *hash == expected
        )
    })
}

/// Load `canonical` at content version A and observe its whole hash;
/// anchor that the four `(canonical, name)`-keyed producers'
/// `engine_fact_signature_for_exported_type` signature builds for the
/// observed-current case before any edit.
fn load_and_observe_keyed(host: &VerterHost, canonical: &str) -> [u8; 16] {
    upsert(host, canonical, &keyed_source_with_sibling("number"));
    host.ensure_indexed_ready(canonical)
        .expect("keyed canonical IndexedReady materialises")
        .whole_hash
}

/// `ImportedRegistryDb`'s producer signature builder is
/// provenance-pure: `resolve_imported_registry_symbol` observes the
/// keyed canonical's content version at the value source and threads
/// it into `engine_fact_signature_for_exported_type`. A STALE observed
/// hash (the keyed canonical was edited after the observation) yields
/// `None` — shared-cache admission is refused — where a pre-fix
/// builder that re-reads current content roots the entry on the
/// post-edit hash.
#[test]
fn imported_registry_db_signature_builder_is_provenance_pure() {
    use crate::resolver_core::component_meta_query_engine::engine_fact_signature_for_exported_type;

    let host = VerterHost::new_standalone(HostConfig::default());
    let c = "/provenance_qdb/imported.ts";
    let observed_h1 = load_and_observe_keyed(&host, c);
    let ctx: &dyn ResolverContext = &host;

    // Step 2 — anchor non-vacuity: observed == current builds `Some`
    // rooted on `H1`.
    let anchored = engine_fact_signature_for_exported_type(ctx, c, "Probe", observed_h1)
        .expect("observed-current signature builds");
    assert!(
        signature_roots_whole_hash(&anchored, c, observed_h1),
        "anchor: the signature for the observed-current case must root on H1",
    );

    // Step 3 — edit the keyed canonical so current becomes H2.
    upsert_skip_drain(&host, c, &keyed_source_with_sibling("string"));
    let current_h2 = host.ensure_indexed_ready(c).expect("re-indexed").whole_hash;
    assert_ne!(
        observed_h1, current_h2,
        "the edit must shift the whole hash"
    );

    // Step 4 — the STALE observed hash refuses admission.
    let ctx2: &dyn ResolverContext = &host;
    assert!(
        engine_fact_signature_for_exported_type(ctx2, c, "Probe", observed_h1).is_none(),
        "ImportedRegistryDb's signature builder MUST return None for the STALE observed \
         hash H1 after the keyed canonical was edited to H2 — the H1 parse-fact registry \
         is drained, so shared-cache admission is refused. A pre-fix builder re-reads \
         authoritative_current_content_hash, resolves H2's registry, and returns Some \
         rooted on H2.",
    );

    // Step 5 — the CURRENT observed hash still builds, proving step 4
    // is the stale-observation refusal, not a broken builder.
    let current_sig = engine_fact_signature_for_exported_type(ctx2, c, "Probe", current_h2)
        .expect("current-observed signature still builds");
    assert!(
        signature_roots_whole_hash(&current_sig, c, current_h2),
        "the current-observed signature must root on H2 — confirming step 4's None is \
         the stale-observation refusal",
    );
}

/// `DeclarationLookupDb`'s producer signature builder is
/// provenance-pure: `resolve_type_declaration` observes the keyed
/// canonical's content version at the value source and threads it into
/// `engine_fact_signature_for_exported_type`.
#[test]
fn declaration_lookup_db_signature_builder_is_provenance_pure() {
    use crate::resolver_core::component_meta_query_engine::engine_fact_signature_for_exported_type;

    let host = VerterHost::new_standalone(HostConfig::default());
    let c = "/provenance_qdb/decl.ts";
    let observed_h1 = load_and_observe_keyed(&host, c);
    let ctx: &dyn ResolverContext = &host;

    let anchored = engine_fact_signature_for_exported_type(ctx, c, "Probe", observed_h1)
        .expect("observed-current signature builds");
    assert!(
        signature_roots_whole_hash(&anchored, c, observed_h1),
        "anchor: the signature for the observed-current case must root on H1",
    );

    upsert_skip_drain(&host, c, &keyed_source_with_sibling("string"));
    let current_h2 = host.ensure_indexed_ready(c).expect("re-indexed").whole_hash;
    assert_ne!(
        observed_h1, current_h2,
        "the edit must shift the whole hash"
    );

    let ctx2: &dyn ResolverContext = &host;
    assert!(
        engine_fact_signature_for_exported_type(ctx2, c, "Probe", observed_h1).is_none(),
        "DeclarationLookupDb's signature builder MUST return None for the STALE observed \
         hash H1 after the keyed canonical was edited to H2. A pre-fix builder re-reads \
         current content and returns Some rooted on H2.",
    );

    let current_sig = engine_fact_signature_for_exported_type(ctx2, c, "Probe", current_h2)
        .expect("current-observed signature still builds");
    assert!(
        signature_roots_whole_hash(&current_sig, c, current_h2),
        "the current-observed signature must root on H2",
    );
}

/// `ResolvabilityDb`'s producer signature builder is provenance-pure:
/// `can_resolve_registry_symbol` observes the keyed canonical's
/// content version at the value source and threads it into
/// `engine_fact_signature_for_exported_type`.
#[test]
fn resolvability_db_signature_builder_is_provenance_pure() {
    use crate::resolver_core::component_meta_query_engine::engine_fact_signature_for_exported_type;

    let host = VerterHost::new_standalone(HostConfig::default());
    let c = "/provenance_qdb/resolvable.ts";
    let observed_h1 = load_and_observe_keyed(&host, c);
    let ctx: &dyn ResolverContext = &host;

    let anchored = engine_fact_signature_for_exported_type(ctx, c, "Probe", observed_h1)
        .expect("observed-current signature builds");
    assert!(
        signature_roots_whole_hash(&anchored, c, observed_h1),
        "anchor: the signature for the observed-current case must root on H1",
    );

    upsert_skip_drain(&host, c, &keyed_source_with_sibling("string"));
    let current_h2 = host.ensure_indexed_ready(c).expect("re-indexed").whole_hash;
    assert_ne!(
        observed_h1, current_h2,
        "the edit must shift the whole hash"
    );

    let ctx2: &dyn ResolverContext = &host;
    assert!(
        engine_fact_signature_for_exported_type(ctx2, c, "Probe", observed_h1).is_none(),
        "ResolvabilityDb's signature builder MUST return None for the STALE observed \
         hash H1 after the keyed canonical was edited to H2. A pre-fix builder re-reads \
         current content and returns Some rooted on H2.",
    );

    let current_sig = engine_fact_signature_for_exported_type(ctx2, c, "Probe", current_h2)
        .expect("current-observed signature still builds");
    assert!(
        signature_roots_whole_hash(&current_sig, c, current_h2),
        "the current-observed signature must root on H2",
    );
}

/// `OwnerCollectionDb`'s producer signature builder is
/// provenance-pure: `owner_collection_expr` observes the keyed owner
/// canonical's prepared decl AND its content version in one
/// `observed_prepared_type_decl` read, then threads the observed hash
/// into `engine_fact_signature_for_exported_type`.
#[test]
fn owner_collection_db_signature_builder_is_provenance_pure() {
    use crate::resolver_core::component_meta_query_engine::engine_fact_signature_for_exported_type;

    let host = VerterHost::new_standalone(HostConfig::default());
    let c = "/provenance_qdb/owner.ts";
    let observed_h1 = load_and_observe_keyed(&host, c);
    let ctx: &dyn ResolverContext = &host;

    let anchored = engine_fact_signature_for_exported_type(ctx, c, "Probe", observed_h1)
        .expect("observed-current signature builds");
    assert!(
        signature_roots_whole_hash(&anchored, c, observed_h1),
        "anchor: the signature for the observed-current case must root on H1",
    );

    upsert_skip_drain(&host, c, &keyed_source_with_sibling("string"));
    let current_h2 = host.ensure_indexed_ready(c).expect("re-indexed").whole_hash;
    assert_ne!(
        observed_h1, current_h2,
        "the edit must shift the whole hash"
    );

    let ctx2: &dyn ResolverContext = &host;
    assert!(
        engine_fact_signature_for_exported_type(ctx2, c, "Probe", observed_h1).is_none(),
        "OwnerCollectionDb's signature builder MUST return None for the STALE observed \
         hash H1 after the keyed canonical was edited to H2. A pre-fix builder re-reads \
         current content and returns Some rooted on H2.",
    );

    let current_sig = engine_fact_signature_for_exported_type(ctx2, c, "Probe", current_h2)
        .expect("current-observed signature still builds");
    assert!(
        signature_roots_whole_hash(&current_sig, c, current_h2),
        "the current-observed signature must root on H2",
    );
}

/// `PreparedMemberDb`'s producer signature builder is provenance-pure:
/// `project_prepared_requested_member_from_symbol` observes the keyed
/// canonical's content version once at the value source and threads it
/// (via `publish_prepared_member_to_host_db`) into
/// `engine_fact_signature_for_canonical_member`. A STALE observed hash
/// yields `None` — the `MemberPresence` / `Member` parse facts cannot
/// be recovered for the drained content version.
#[test]
fn prepared_member_db_signature_builder_is_provenance_pure() {
    use crate::resolver_core::component_meta_query_engine::engine_fact_signature_for_canonical_member;

    let host = VerterHost::new_standalone(HostConfig::default());
    let c = "/provenance_qdb/pmem.ts";
    let observed_h1 = load_and_observe_keyed(&host, c);
    let ctx: &dyn ResolverContext = &host;

    // The keyed member is `Probe.a` — `keyed_source_with_sibling`
    // declares `interface Probe { a: number; b: string; }`.
    let anchored = engine_fact_signature_for_canonical_member(ctx, c, "Probe", "a", observed_h1)
        .expect("observed-current signature builds");
    assert!(
        signature_roots_whole_hash(&anchored, c, observed_h1),
        "anchor: the signature for the observed-current case must root on H1",
    );

    upsert_skip_drain(&host, c, &keyed_source_with_sibling("string"));
    let current_h2 = host.ensure_indexed_ready(c).expect("re-indexed").whole_hash;
    assert_ne!(
        observed_h1, current_h2,
        "the edit must shift the whole hash"
    );

    let ctx2: &dyn ResolverContext = &host;
    assert!(
        engine_fact_signature_for_canonical_member(ctx2, c, "Probe", "a", observed_h1).is_none(),
        "PreparedMemberDb's signature builder MUST return None for the STALE observed \
         hash H1 after the keyed canonical was edited to H2 — the H1 MemberPresence / \
         Member parse-fact registry is drained. A pre-fix builder re-reads current \
         content via parse_fact_ref and returns Some rooted on H2.",
    );

    let current_sig = engine_fact_signature_for_canonical_member(ctx2, c, "Probe", "a", current_h2)
        .expect("current-observed signature still builds");
    assert!(
        signature_roots_whole_hash(&current_sig, c, current_h2),
        "the current-observed signature must root on H2",
    );
}

/// `PreparedTargetDb`'s producer signature builder is provenance-pure:
/// `resolve_prepared_surface_target` observes BOTH keyed canonicals'
/// content versions at the value source and threads them into
/// `engine_fact_signature_for_prepared_target`. A STALE observed hash
/// for either keyed canonical yields `None`.
#[test]
fn prepared_target_db_signature_builder_is_provenance_pure() {
    use crate::resolver_core::component_meta_query_engine::engine_fact_signature_for_prepared_target;

    let host = VerterHost::new_standalone(HostConfig::default());
    let c = "/provenance_qdb/ptgt.ts";
    let observed_h1 = load_and_observe_keyed(&host, c);
    let ctx: &dyn ResolverContext = &host;

    // Active scope and declaring canonical are the same file here, so
    // one observation roots both self-roots.
    let anchored = engine_fact_signature_for_prepared_target(
        ctx,
        c,
        "Probe",
        observed_h1,
        c,
        "Probe",
        observed_h1,
    )
    .expect("observed-current signature builds");
    assert!(
        signature_roots_whole_hash(&anchored, c, observed_h1),
        "anchor: the signature for the observed-current case must root on H1",
    );

    upsert_skip_drain(&host, c, &keyed_source_with_sibling("string"));
    let current_h2 = host.ensure_indexed_ready(c).expect("re-indexed").whole_hash;
    assert_ne!(
        observed_h1, current_h2,
        "the edit must shift the whole hash"
    );

    let ctx2: &dyn ResolverContext = &host;
    assert!(
        engine_fact_signature_for_prepared_target(
            ctx2,
            c,
            "Probe",
            observed_h1,
            c,
            "Probe",
            observed_h1,
        )
        .is_none(),
        "PreparedTargetDb's signature builder MUST return None for the STALE observed \
         hash H1 after the keyed canonical was edited to H2. A pre-fix builder re-reads \
         current content for both self-roots and returns Some rooted on H2.",
    );

    let current_sig = engine_fact_signature_for_prepared_target(
        ctx2, c, "Probe", current_h2, c, "Probe", current_h2,
    )
    .expect("current-observed signature still builds");
    assert!(
        signature_roots_whole_hash(&current_sig, c, current_h2),
        "the current-observed signature must root on H2",
    );
}

// ---------------------------------------------------------------------------
// Producer-level overlay-discrimination tests — P1-A / P1-B.
//
// The six `*_signature_builder_is_provenance_pure` tests above call the
// signature *helpers* in isolation with a hand-passed hash, so they cannot
// catch a producer that sources the observed hash from a base-host-only oracle
// (`shallow_file_state`, not view-aware) instead of the view-aware
// `authoritative_current_content_hash` / the prepared-decl bundle provenance.
//
// Each test below drives an actual producer METHOD through a
// `SessionResolverContext` carrying an overlay (content `O`) over a base file
// (content `B`), `O != B`. The producer publishes into the canonical-keyed
// (NOT view-qualified) shared query-cache DB. A fresh base-context producer
// then issues a follow-up request against the SAME slot.
//
// Discrimination property (FAILS pre-fix, PASSES post-fix):
//
//  - Post-fix the producer observes the OVERLAY hash `O` (the view-aware
//    oracle, or the overlay-aware prepared-decl bundle), so the overlay-derived
//    entry's self-root `FileWholeHash` carries `O`. The base follow-up's
//    `validate_fact_signature_with_self_roots` checks that self-root against
//    the base view's whole-hash `B`; `O != B` mismatches, the warm read MISSES,
//    and the base producer cold-recomputes the base-content value.
//  - Pre-fix the producer observes `shallow_file_state(canonical).whole_hash`,
//    which a `SessionResolverContext` delegates to the BASE host — so the
//    overlay-derived entry's self-root carries the base hash `B`. The base
//    follow-up validates `B`-vs-`B`, WARM-HITS, and is served the overlay
//    value. The base producer returns overlay-content data and the assertion
//    on base-content data trips.
//
// The discriminator is the base producer's RETURN VALUE: the base and overlay
// sources are deliberately written so the producer outputs differ (the type
// `Probe` carries a `baseMember` field in the base source and an
// `overlayMember` field in the overlay source).
//
// Cache coverage. The three producer tests below cover the query caches whose
// producer is externally callable, whose cold value-compute is sourced from
// the prepared-decl bundle (view-isolated), AND whose producer admits a
// torn/base-rooted entry pre-fix so the leak is observable: `ResolvabilityDb`,
// `PreparedSurfaceDb`, `PreparedMemberDb`. The fourth test pins
// `observed_prepared_type_decl` itself — the single-artifact observation point
// shared by the `OwnerCollectionDb` producer. The remaining caches' producers
// are not amenable to a producer-level overlay test: `DeclarationLookupDb` /
// `RoutedExprSurfaceDb` (and the imported-registry resolver) recover their
// value through shallow-metadata / dispatch reads that consult the
// non-content-pinned `FileArtifactStore::get_any`, which itself returns the
// overlay candidate to a base recompute (a separate pre-existing
// content-pinning gap — see this file's earlier `[debt]` note), so a
// producer-level test cannot isolate the self-root fix; `OwnerCollectionDb`'s
// producer refuses admission of the torn entry pre-fix (see the note above the
// `ResolvabilityDb` test); `PreparedTargetDb`'s producer is `pub(super)` and
// not reachable from this module. Their round-6 producer hash-source change
// (the identical base-only `shallow_file_state` → view-aware
// `authoritative_current_content_hash` substitution) is covered by the
// `*_signature_builder_is_provenance_pure` builder tests above and the
// `central_fact_signature_helpers_are_provenance_pure` architecture guard.

/// Base source for an overlay-discrimination fixture: type `Probe`
/// carries a `baseMember` field.
fn overlay_disc_base_source() -> &'static str {
    "export interface Probe { baseMember: number; }\n"
}

/// Overlay source for an overlay-discrimination fixture: type `Probe`
/// carries an `overlayMember` field — deliberately different bytes (and
/// a different member) from the base so the producer output and the
/// content hash both differ.
fn overlay_disc_overlay_source() -> &'static str {
    "export interface Probe { overlayMember: string; }\n"
}

/// Build the overlay-discrimination fixture: a host with `canonical`
/// materialised at the base source, plus an [`OverlaidView`] masking
/// `canonical` with the overlay source. The overlay `IndexedReady`
/// candidate is materialised under the overlay hash. Returns the
/// `Arc<VerterHost>`, the view, and the two distinct content hashes.
fn overlay_disc_fixture(
    canonical: &str,
) -> (
    Arc<VerterHost>,
    crate::session_view::OverlaidView,
    [u8; 16],
    [u8; 16],
) {
    use crate::session_view::SessionView;
    use rustc_hash::FxHashMap;

    let host = VerterHost::new_standalone(HostConfig::default());
    upsert(&host, canonical, overlay_disc_base_source());
    let base_hash = host
        .ensure_indexed_ready(canonical)
        .expect("base IndexedReady materialises")
        .whole_hash;
    let host = Arc::new(host);

    let overlay_source: Arc<str> = Arc::from(overlay_disc_overlay_source());
    let mut overlays: FxHashMap<String, Arc<str>> = FxHashMap::default();
    overlays.insert(canonical.to_string(), Arc::clone(&overlay_source));
    let view = crate::session_view::OverlaidView::new(Arc::clone(&host), overlays);

    let overlay_hash = view
        .overlay_content_hash_for(canonical)
        .expect("OverlaidView reports an overlay content hash for the masked canonical");
    assert_ne!(
        overlay_hash, base_hash,
        "fixture invariant: the overlay source differs from the base, so the overlay \
         content hash differs — otherwise the base/overlay entries are indistinguishable",
    );

    host.materialize_overlay_indexed_ready(canonical, &overlay_source, overlay_hash)
        .expect("overlay IndexedReady materialises");

    (host, view, base_hash, overlay_hash)
}

// `OwnerCollectionDb`'s producer (`owner_collection_expr`) is not
// covered by a producer-level overlay test here: pre-fix its closure
// builds the signature through `parse_fact_ref_for_observed_current_content`
// keyed on the (base-only-sourced) observed hash, and after the overlay
// `IndexedReady` candidate is materialised the base-hash artifact is no
// longer the slot's current-content candidate, so the parse-fact lookup
// returns `None` and the pre-fix closure refuses admission anyway — the
// torn entry is never published, so a producer-level test cannot observe
// a leak. The P1-B torn-read defect in `OwnerCollectionDb`'s observation
// point is instead pinned directly by
// `observed_prepared_type_decl_is_single_artifact_and_view_aware` below,
// which asserts the `(decl, whole_hash)` pair `observed_prepared_type_decl`
// returns is single-artifact-consistent and view-correct.

/// `ResolvabilityDb` — producer-level overlay discrimination (P1-A).
///
/// Drives `can_resolve_registry_symbol`. The base source declares
/// `Probe`; the overlay source does NOT (it declares `Other` instead) —
/// so the resolvability boolean differs between the views. The base
/// follow-up MUST recompute `true`; a leaked overlay entry would carry
/// `false`.
#[test]
fn resolvability_db_producer_overlay_discrimination() {
    use crate::resolver_core::component_meta_query_engine::ComponentMetaQueryEngine;
    use crate::resolver_core::SessionResolverContext;
    use crate::session_view::SessionView;
    use rustc_hash::FxHashMap;

    let canonical = "/overlay_disc/resolvable.ts";
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert(&host, canonical, "export interface Probe { a: number; }\n");
    host.ensure_indexed_ready(canonical)
        .expect("base IndexedReady materialises");
    let host = Arc::new(host);

    // The overlay does NOT declare `Probe` — it declares `Other`.
    let overlay_source: Arc<str> = Arc::from("export interface Other { a: number; }\n");
    let mut overlays: FxHashMap<String, Arc<str>> = FxHashMap::default();
    overlays.insert(canonical.to_string(), Arc::clone(&overlay_source));
    let view = crate::session_view::OverlaidView::new(Arc::clone(&host), overlays);
    let overlay_hash = view
        .overlay_content_hash_for(canonical)
        .expect("overlay content hash present");
    host.materialize_overlay_indexed_ready(canonical, &overlay_source, overlay_hash)
        .expect("overlay IndexedReady materialises");

    let overlay_ctx = SessionResolverContext::new(&host, &view);
    let mut overlay_engine = ComponentMetaQueryEngine::new(&overlay_ctx);
    let overlay_resolvable = overlay_engine.can_resolve_registry_symbol(canonical, "Probe", None);
    assert!(
        !overlay_resolvable,
        "fixture invariant: the overlay source does not declare `Probe`, so the \
         overlay producer must resolve `Probe` as NOT resolvable",
    );

    let base_ctx: &dyn ResolverContext = host.as_ref();
    let mut base_engine = ComponentMetaQueryEngine::new(base_ctx);
    let base_resolvable = base_engine.can_resolve_registry_symbol(canonical, "Probe", None);
    assert!(
        base_resolvable,
        "ResolvabilityDb LEAKED an overlay-session entry to a base request. \
         `can_resolve_registry_symbol` must observe the keyed canonical's content \
         version through the view-aware `authoritative_current_content_hash`, so the \
         overlay entry (`Probe` NOT resolvable) roots on the overlay hash and a base \
         request mismatches it. A producer reading the base-only `shallow_file_state` \
         roots the entry on the base hash; the base request then warm-hits the \
         overlay `false` even though the base source declares `Probe`.",
    );
}

/// `PreparedSurfaceDb` — producer-level overlay discrimination (P1-A).
///
/// Drives `cached_prepared_root_surface`; the value is the projected
/// `ProjectedSurface` of `Probe`, whose member set is the discriminator.
#[test]
fn prepared_surface_db_producer_overlay_discrimination() {
    use crate::resolver_core::component_meta_query_engine::ComponentMetaQueryEngine;
    use crate::resolver_core::SessionResolverContext;

    let canonical = "/overlay_disc/prepared_surface.ts";
    let (host, view, _base_hash, _overlay_hash) = overlay_disc_fixture(canonical);

    let overlay_ctx = SessionResolverContext::new(&host, &view);
    let mut overlay_engine = ComponentMetaQueryEngine::new(&overlay_ctx);
    let overlay_surface = overlay_engine
        .cached_prepared_root_surface(canonical, "Probe")
        .expect("the overlay producer projects Probe's surface");
    assert!(
        overlay_surface
            .members
            .iter()
            .any(|m| m.name == "overlayMember"),
        "fixture invariant: the overlay producer must project the overlay source's \
         `overlayMember` field",
    );

    let base_ctx: &dyn ResolverContext = host.as_ref();
    let mut base_engine = ComponentMetaQueryEngine::new(base_ctx);
    let base_surface = base_engine
        .cached_prepared_root_surface(canonical, "Probe")
        .expect("the base producer projects Probe's surface");
    assert!(
        base_surface.members.iter().any(|m| m.name == "baseMember"),
        "PreparedSurfaceDb LEAKED an overlay-session entry to a base request. \
         `project_prepared_surface_from_symbol` must observe the scope canonical's \
         content version through the view-aware `authoritative_current_content_hash`, \
         so the overlay surface roots on the overlay hash and a base request \
         mismatches it. A producer reading the base-only `shallow_file_state` roots \
         the entry on the base hash; the base request warm-hits the overlay surface.",
    );
    assert!(
        !base_surface
            .members
            .iter()
            .any(|m| m.name == "overlayMember"),
        "the base producer must not surface the overlay `overlayMember` field",
    );
}

/// `PreparedMemberDb` — producer-level overlay discrimination (P1-A).
///
/// Drives `project_prepared_requested_member_from_symbol`. The base
/// source declares `Probe.shared` as `number`; the overlay declares it
/// as `string` — the projected member type is the discriminator.
#[test]
fn prepared_member_db_producer_overlay_discrimination() {
    use crate::resolver_core::component_meta_query_engine::ComponentMetaQueryEngine;
    use crate::resolver_core::SessionResolverContext;
    use crate::session_view::SessionView;
    use rustc_hash::{FxHashMap, FxHashSet};

    let canonical = "/overlay_disc/prepared_member.ts";
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert(
        &host,
        canonical,
        "export interface Probe { shared: number; }\n",
    );
    host.ensure_indexed_ready(canonical)
        .expect("base IndexedReady materialises");
    let host = Arc::new(host);

    // The overlay declares `Probe.shared` with a different type.
    let overlay_source: Arc<str> = Arc::from("export interface Probe { shared: string; }\n");
    let mut overlays: FxHashMap<String, Arc<str>> = FxHashMap::default();
    overlays.insert(canonical.to_string(), Arc::clone(&overlay_source));
    let view = crate::session_view::OverlaidView::new(Arc::clone(&host), overlays);
    let overlay_hash = view
        .overlay_content_hash_for(canonical)
        .expect("overlay content hash present");
    host.materialize_overlay_indexed_ready(canonical, &overlay_source, overlay_hash)
        .expect("overlay IndexedReady materialises");

    let overlay_ctx = SessionResolverContext::new(&host, &view);
    let mut overlay_engine = ComponentMetaQueryEngine::new(&overlay_ctx);
    let mut active: FxHashSet<(String, String)> = FxHashSet::default();
    let overlay_member = overlay_engine
        .project_prepared_requested_member_from_symbol(
            canonical,
            "Probe",
            "shared",
            &FxHashMap::default(),
            &mut active,
        )
        .expect("the overlay producer projects Probe.shared");
    assert!(
        matches!(
            &overlay_member.ty,
            TypeExpr::Primitive(verter_type_expr::PrimitiveName::String)
        ),
        "fixture invariant: the overlay producer must see `Probe.shared` as `string`, \
         got {:?}",
        overlay_member.ty,
    );

    let base_ctx: &dyn ResolverContext = host.as_ref();
    let mut base_engine = ComponentMetaQueryEngine::new(base_ctx);
    let mut base_active: FxHashSet<(String, String)> = FxHashSet::default();
    let base_member = base_engine
        .project_prepared_requested_member_from_symbol(
            canonical,
            "Probe",
            "shared",
            &FxHashMap::default(),
            &mut base_active,
        )
        .expect("the base producer projects Probe.shared");
    assert!(
        matches!(
            &base_member.ty,
            TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number)
        ),
        "PreparedMemberDb LEAKED an overlay-session entry to a base request. \
         `project_prepared_requested_member_from_symbol` must observe the scope \
         canonical's content version through the view-aware \
         `authoritative_current_content_hash`, so the overlay member roots on the \
         overlay hash and a base request mismatches it. A producer reading the \
         base-only `shallow_file_state` roots the entry on the base hash; the base \
         request warm-hits the overlay `string` even though the base source \
         declares `Probe.shared` as `number`. Base member type was {:?}.",
        base_member.ty,
    );
}

/// `observed_prepared_type_decl` is single-artifact AND view-aware
/// (P1-B + P1-A).
///
/// `observed_prepared_type_decl` is the observation point for the
/// `OwnerCollectionDb` producer: it must return the prepared decl AND
/// the content version it was materialised from sourced from ONE
/// prepared-decl bundle, so the pair cannot tear and the hash is
/// view-correct.
///
/// Discrimination property: driven through a `SessionResolverContext`
/// with an overlay, the returned `whole_hash` MUST equal the overlay
/// content hash — the version the overlay-aware prepared-decl bundle
/// (and therefore the returned `decl`) was built from. Pre-fix the
/// accessor read the decl via the overlay-aware `prepared_type_decl`
/// but the hash via the base-only `shallow_file_state`, so it returned
/// a TORN pair: the overlay decl bundled with the BASE content hash.
/// This test asserts `whole_hash == overlay_hash` and trips on the
/// torn base-hash pre-fix result.
#[test]
fn observed_prepared_type_decl_is_single_artifact_and_view_aware() {
    use crate::resolver_core::component_meta_query_engine::ComponentMetaQueryEngine;
    use crate::resolver_core::SessionResolverContext;

    let canonical = "/overlay_disc/observed_prepared.ts";
    let (host, view, base_hash, overlay_hash) = overlay_disc_fixture(canonical);

    let overlay_ctx = SessionResolverContext::new(&host, &view);
    let mut overlay_engine = ComponentMetaQueryEngine::new(&overlay_ctx);
    let observed = overlay_engine
        .observed_prepared_type_decl(canonical, "Probe")
        .expect("observed_prepared_type_decl resolves through the overlay bundle");

    // The decl must be the OVERLAY decl — its member index carries
    // `overlayMember`, not `baseMember`.
    let decl = observed
        .decl
        .as_ref()
        .expect("the overlay bundle declares Probe");
    assert!(
        decl.member_index.contains_key("overlayMember"),
        "fixture invariant: the overlay-aware prepared-decl bundle must yield the \
         overlay `Probe` (member `overlayMember`)",
    );

    // The discriminating assertion: the observed hash is the OVERLAY
    // content hash — the version the bundle (and the decl above) were
    // built from. A torn read that sourced the hash from the base-only
    // `shallow_file_state` returns the base hash here.
    assert_eq!(
        observed.whole_hash, overlay_hash,
        "observed_prepared_type_decl returned a TORN (decl, whole_hash) pair: the decl \
         is the overlay `Probe` but `whole_hash` is not the overlay content version. \
         The accessor MUST source the decl AND the hash from ONE prepared-decl bundle \
         (`PreparedTypeDeclCache::defining_content_hash`), which is overlay-aware. \
         Reading the hash from the base-only `shallow_file_state` yields a torn pair \
         and roots the `OwnerCollectionDb` entry on a content version the value was \
         not built from.",
    );
    assert_ne!(
        observed.whole_hash, base_hash,
        "the observed hash must NOT be the base content hash — that is the torn-read \
         (overlay decl + base hash) defect this accessor's single-artifact provenance \
         closes",
    );
}

// ---------------------------------------------------------------------------
// Single-observation materialize scope identity.
//
// The materialize-memo publish site needs the scope's content version
// for two consumers that MUST agree: the `NodeScopeId::File` the
// materialiser lowers against, and the `MaterializeMemoDb` signature
// self-root. Pre-fix the publish site read TWO separate oracles
// (`shallow_file_state` for the scope id, `authoritative_current_content_hash`
// for the signature) — an edit between the two reads roots a value
// lowered under `H1` on a signature self-rooted at `H2`.
// `observe_materialize_scope` collapses both onto ONE `Arc<IndexedReady>`.
// ---------------------------------------------------------------------------

/// Atomicity — the materialiser's lowering `NodeScopeId.whole_hash` AND
/// the materialize-memo signature self-root hash come from ONE
/// `observe_materialize_scope` observation.
///
/// Discrimination property: the test takes the single observation, then
/// (a) runs the production publish path and reads the lowered `DeclRef`
/// node's `NodeScopeId::File` scope `whole_hash` out of the semantic
/// graph, and (b) builds the materialize-memo fact signature through
/// `engine_fact_signature_for_materialize_memo` and extracts the
/// keyed-scope self-root `FileWholeHash`. Both MUST equal
/// `observe_materialize_scope(scope).whole_hash()` — a single source.
///
/// Pre-fix the publish site sourced the lowering hash from
/// `shallow_file_state` and the signature hash from
/// `authoritative_current_content_hash` — two oracles, separately read,
/// tearable. They happen to agree on a steady-state scheduler-backed
/// file, so this test does not discriminate by hash *inequality*;
/// instead it pins the architectural invariant directly — both hashes
/// are identical AND both equal the one observation's `whole_hash()`.
/// The race-window discriminators
/// (`materialize_memo_db_scope_self_root_carries_observed_hash_not_current`,
/// `materialize_memo_db_scope_edit_in_race_window_rejects_stale_entry_end_to_end`)
/// drive the tearing edit; this test is the steady-state contract pin.
#[test]
fn materialize_memo_scope_lowering_and_signature_root_share_one_observation() {
    use crate::resolver_core::component_meta_query_engine::engine_fact_signature_for_materialize_memo;
    use crate::resolver_core::ComponentMetaQueryEngine;
    use crate::semantic_query::{NodeScopeId, ProjectionMode, SemanticNodeId};

    let host = VerterHost::new_standalone(HostConfig::default());
    let scope = "/self_root_obs/atomic_scope.ts";
    upsert(&host, scope, "export interface Probe { a: number; }\n");
    assert!(
        host.ensure_indexed_ready(scope).is_some(),
        "scope IndexedReady materialises",
    );

    let ctx: &dyn ResolverContext = &host;
    // The ONE observation the publish site takes.
    let observed_scope = observe_scope(ctx, scope);
    let observation_hash = observed_scope.whole_hash();
    assert_ne!(
        observation_hash, [0u8; 16],
        "fixture invariant: the observation carries a real, non-zero whole hash",
    );

    // (a) Run the production publish path and read the lowered
    // `DeclRef` node's `NodeScopeId::File` scope hash.
    let probe_expr = TypeExpr::Ref {
        name: Arc::from("Probe"),
        type_arguments: Arc::from(Vec::<TypeExpr>::new()),
    };
    let mut engine = ComponentMetaQueryEngine::new(&host);
    let _ =
        crate::meta_resolve::materialize::materialize_component_meta_type_expr_until_stable_full(
            &probe_expr,
            scope,
            ProjectionMode::Navigate,
            &mut engine,
        );
    let graph = host.project_type_store().semantic_graph();
    let lowering_scope_hash = (0..graph.node_count() as u64)
        .find_map(|i| {
            let id = SemanticNodeId(i);
            let data = graph.node_data(id)?;
            let names_probe = matches!(
                &*data,
                crate::semantic_query::SemanticNodeData::DeclRef { identity }
                    if identity.canonical_id.as_ref() == scope
                        && identity.decl_name.as_ref() == "Probe"
            );
            if !names_probe {
                return None;
            }
            match graph.node_scope(id)? {
                NodeScopeId::File { whole_hash, .. } => Some(whole_hash),
                NodeScopeId::Global => None,
            }
        })
        .expect(
            "the Navigate-mode materialisation lowers `Probe` to a `NodeScopeId::File` DeclRef",
        );

    // (b) Build the materialize-memo fact signature and extract the
    // keyed-scope self-root `FileWholeHash`.
    let signature = engine_fact_signature_for_materialize_memo(
        &observed_scope,
        observed_scope
            .syntactic_export_set
            .clone()
            .expect("scope SyntacticExportSet parse fact recoverable"),
        &empty_dep_signature(),
    )
    .expect("a dep-free materialize-memo signature is admissible");
    let signature_self_root_hash = signature
        .iter()
        .find_map(|f| match f {
            FactVersionRef::FileWholeHash { canonical_id, hash } if canonical_id == scope => {
                Some(*hash)
            }
            _ => None,
        })
        .expect("the materialize-memo signature MUST carry a keyed-scope self-root FileWholeHash");

    assert_eq!(
        lowering_scope_hash, observation_hash,
        "the materialiser's lowering `NodeScopeId::File` whole hash MUST equal the single \
         `observe_materialize_scope` observation's `whole_hash()` — the publish site \
         builds the `NodeScopeId` from exactly that observation",
    );
    assert_eq!(
        signature_self_root_hash, observation_hash,
        "the materialize-memo signature's keyed-scope self-root `FileWholeHash` MUST equal \
         the single observation's `whole_hash()` — the publish site threads the same \
         observation into `engine_fact_signature_for_materialize_memo`",
    );
    assert_eq!(
        lowering_scope_hash, signature_self_root_hash,
        "the lowering scope hash and the signature self-root hash MUST be identical — they \
         descend from ONE `Arc<IndexedReady>`, not from two separately-read oracles. A \
         publish site that sourced the lowering hash from `shallow_file_state` and the \
         signature hash from `authoritative_current_content_hash` reads two oracles that \
         can tear under a mid-flight edit.",
    );
}

/// Overlay view-correctness — under a `SessionResolverContext` carrying
/// an overlay, `observe_materialize_scope` returns the OVERLAY
/// `IndexedReady` (the overlay content hash), not the base one.
///
/// Discrimination property: an `OverlaidView` masks `canonical` with an
/// overlay source whose content hash (`overlay_hash`) differs from the
/// base (`base_hash`). Driven through a `SessionResolverContext`,
/// `observe_materialize_scope(canonical).whole_hash()` MUST equal
/// `overlay_hash`. The base-host `observe_materialize_scope` (no
/// overlay) MUST equal `base_hash`. A `SessionResolverContext` impl
/// that delegated straight to the base host — or sourced the artifact
/// from the base-only `shallow_file_state` — returns `base_hash` here
/// and the `assert_eq!(.., overlay_hash)` trips.
///
/// This is the LSP edit-in-overlay path: an overlay materialize-memo
/// entry roots on the overlay version, so it hits for the same overlay
/// content and never cross-validates a base request.
#[test]
fn observe_materialize_scope_is_overlay_view_correct() {
    use crate::resolver_core::SessionResolverContext;

    let canonical = "/overlay_disc/observe_materialize_scope.ts";
    let (host, view, base_hash, overlay_hash) = overlay_disc_fixture(canonical);

    // Base-host observation — no overlay → the base content hash.
    let base_ctx: &dyn ResolverContext = host.as_ref();
    let base_observation = base_ctx
        .observe_materialize_scope(canonical)
        .expect("base-host observe_materialize_scope resolves the base artifact");
    assert_eq!(
        base_observation.whole_hash(),
        base_hash,
        "the base-host `observe_materialize_scope` MUST observe the base content version",
    );

    // Overlay-session observation — the overlay content hash.
    let overlay_ctx = SessionResolverContext::new(&host, &view);
    let overlay_observation = ResolverContext::observe_materialize_scope(&overlay_ctx, canonical)
        .expect("overlay-session observe_materialize_scope resolves the overlay-pinned artifact");
    assert_eq!(
        overlay_observation.whole_hash(),
        overlay_hash,
        "the overlay-session `observe_materialize_scope` MUST observe the OVERLAY content \
         version — the overlay `IndexedReady` candidate was prewarmed under the overlay \
         hash. A `SessionResolverContext` impl that delegated to the base host (or read \
         the base-only `shallow_file_state`) observes the base hash, mis-rooting an \
         overlay-derived memo entry on the base version.",
    );
    assert_ne!(
        overlay_observation.whole_hash(),
        base_hash,
        "the overlay observation must NOT carry the base content hash — that torn read \
         would let an overlay-derived memo entry cross-validate for a base request",
    );

    // The pinned `SyntacticExportSet` parse fact descends from the SAME
    // overlay artifact — its canonical names the scope (not a torn
    // base/overlay mix).
    let overlay_export_set = overlay_observation
        .syntactic_export_set
        .clone()
        .expect("the overlay observation carries the scope's SyntacticExportSet parse fact");
    assert_eq!(
        overlay_export_set.canonical_id, canonical,
        "the observation's pinned parse fact MUST describe the keyed scope canonical",
    );
}

/// Evicted / stale refusal — for an evicted scope whose only
/// `FileArtifactStore` artifact is a stale leftover,
/// `observe_materialize_scope` returns `None`, so no `MaterializeMemoDb`
/// entry is admitted; a follow-up request still computes a value.
///
/// Discrimination property: the scope is upserted (creating a live
/// `DerivedRawState` + a `FileArtifactStore` artifact), then evicted —
/// `evict` marks the `DerivedRawState` `evicted` while leaving the
/// `FileArtifactStore` artifact in place. `observe_materialize_scope`
/// MUST return `None`: the scheduler authority refuses (the
/// `DerivedRawState` is evicted) AND the artifact-current authority
/// refuses (the surviving artifact is a stale leftover, detected via
/// the `evicted` flag). The publish closure then `?`-returns `None`, so
/// `get_or_compute` admits nothing and a follow-up request cold-
/// recomputes.
///
/// An artifact-current authority that used a content-agnostic `get_any`
/// (no eviction check) would return `Some` for the stale artifact and
/// admit a stale memo entry — this test FAILS against that
/// (`observe_materialize_scope` would be `Some`, the entry would be
/// admitted, and the follow-up cold closure would not run). The
/// eviction-aware authority returns `None` and this test PASSES.
#[test]
fn observe_materialize_scope_refuses_evicted_stale_artifact() {
    use crate::resolver_core::component_meta_query_engine::engine_fact_signature_for_materialize_memo;
    use crate::semantic_query::ProjectionMode;

    let host = VerterHost::new_standalone(HostConfig::default());
    let scope = "/self_root_obs/evicted_scope.ts";
    upsert(&host, scope, "export type Probe = number;\n");
    assert!(
        host.ensure_indexed_ready(scope).is_some(),
        "scope IndexedReady materialises",
    );

    let ctx: &dyn ResolverContext = &host;
    // Pre-eviction: `observe_materialize_scope` returns Some.
    assert!(
        ctx.observe_materialize_scope(scope).is_some(),
        "fixture invariant: a live scope has a tear-free observation",
    );

    // Evict the scope: `DerivedRawState` is marked evicted; the
    // `FileArtifactStore` artifact survives as a stale leftover.
    host.evict(scope);
    assert!(
        host.project_type_store().indexed().get_any(scope).is_some(),
        "fixture invariant: the evicted scope's stale FileArtifactStore artifact survives",
    );

    // The discriminating assertion: `observe_materialize_scope` MUST
    // return `None` — the surviving artifact is a stale evicted
    // leftover, not a current artifact-backed file. A content-agnostic
    // `get_any` would surface it.
    assert!(
        ctx.observe_materialize_scope(scope).is_none(),
        "observe_materialize_scope MUST return None for an evicted scope whose only \
         FileArtifactStore artifact is a stale leftover — the artifact-current authority \
         distinguishes a current artifact-backed file from a stale evicted leftover via \
         the DerivedRawState `evicted` flag. A content-agnostic `get_any` currentness \
         oracle would surface the stale artifact and admit a stale memo entry.",
    );

    // The publish closure threads the `None` observation through `?`,
    // so `get_or_compute` admits nothing and a follow-up cold-recomputes.
    let db = host.project_type_store().materialize_memo_db();
    let probe_expr = Arc::new(TypeExpr::Ref {
        name: Arc::from("Probe"),
        type_arguments: Arc::from(Vec::<TypeExpr>::new()),
    });
    let key = (
        Arc::<str>::from(scope),
        Arc::clone(&probe_expr),
        ProjectionMode::Expanded,
    );
    let scope_owned = scope.to_string();
    let cold_value = db.get_or_compute(&key, ctx, move || {
        // The production publish closure: a `None` observation refuses
        // shared-cache admission.
        let observed_scope = ctx.observe_materialize_scope(scope_owned.as_str())?;
        let export_set = observed_scope.syntactic_export_set.clone()?;
        let fact_sig = engine_fact_signature_for_materialize_memo(
            &observed_scope,
            export_set,
            &empty_dep_signature(),
        )?;
        Some((materialized("stale", empty_dep_signature()), fact_sig))
    });
    assert!(
        cold_value.is_none(),
        "the publish path threads the None observation through `?`, so get_or_compute \
         declines to insert and returns None",
    );
    assert_eq!(
        db.live_count(),
        0,
        "no MaterializeMemoDb entry may be admitted when the scope observation is refused",
    );

    let ctx2: &dyn ResolverContext = &host;
    let mut cold_ran = false;
    let value = db
        .get_or_compute(&key, ctx2, || {
            cold_ran = true;
            Some((
                materialized("recomputed", empty_dep_signature()),
                empty_fact_signature(),
            ))
        })
        .expect("the follow-up request still computes and returns a value");
    assert!(
        cold_ran,
        "the follow-up request's cold closure MUST run — no entry was admitted, so there \
         is no warm hit to short-circuit it",
    );
    assert!(
        matches!(&value.type_expr, TypeExpr::Unknown { raw } if raw == "recomputed"),
        "the refused-admission path still returns the freshly-computed value to the caller",
    );
}

/// `observe_materialize_scope` MUST NOT surface a STALE artifact for a
/// **live** (non-evicted) scheduler scope whose scheduler-pinned
/// authority merely missed.
///
/// Root cause this test guards: `artifact_current_indexed` (the
/// artifact-current authority `observe_materialize_scope` falls back to
/// after `current_content_pinned_indexed` misses) gated its
/// `FileArtifactStore::get_any` read only on the `DerivedRawState`
/// `evicted` flag. That lumped two distinct cases:
///
/// - a genuinely artifact-only scope with NO `DerivedRawState` entry at
///   all (foreign source / test seed) — `get_any` is appropriate; and
/// - a **live** scheduler scope (a non-evicted `DerivedRawState`) whose
///   `current_content_pinned_indexed` transiently missed — here
///   `get_any` is WRONG: with eager `evict_canonical` retired, a stale
///   older `IndexedReady` can coexist with the live content, and
///   `get_any` returns ANY cached candidate regardless of content hash.
///
/// For the live-scope case `observe_materialize_scope` would then lower
/// and self-root the materialized value under the STALE artifact's
/// `whole_hash` instead of the scheduler's current hash — admitting a
/// mis-rooted `MaterializeMemoDb` entry.
///
/// ## Fixture — a live scope with a planted stale artifact
///
/// A real `.ts` file is upserted through the production path (creating
/// a live, **non-evicted** `DerivedRawState` plus a current
/// `FileArtifactStore` artifact). A synthetic STALE `IndexedReady`
/// (doctored `whole_hash`) is then planted via `FileArtifactStore::insert`
/// — `insert` drains every prior version, so afterwards the store holds
/// ONLY the stale entry while the scheduler still reports the real
/// `whole_hash`. The scope therefore has:
///
/// - a live, non-evicted `DerivedRawState` →
///   `authoritative_current_content_hash` returns the REAL hash;
/// - `current_content_pinned_indexed` pins to that real hash and MISSES
///   (only the `STALE_HASH` artifact is stored);
/// - a permissive `get_any` would surface the stale artifact.
///
/// ## Discrimination property
///
/// - PRE-FIX: `artifact_current_indexed` sees a non-evicted
///   `DerivedRawState`, takes the `get_any` branch, and returns the
///   STALE artifact; `observe_materialize_scope` returns `Some` whose
///   `whole_hash()` is the doctored stale hash. The first assertion
///   (`is_none`) FAILS; were it weakened, the second
///   (`whole_hash() != STALE_HASH`) FAILS.
/// - POST-FIX: `artifact_current_indexed` restricts the `get_any`
///   fallback to canonicals with NO `DerivedRawState` entry at all; a
///   live (non-evicted) entry yields `None`. `observe_materialize_scope`
///   returns `None` — refusing shared-cache admission while the publish
///   site still returns the freshly-computed value.
///
/// The companion `current_content_pinned_indexed_rejects_stale_artifact`
/// pins only that `current_content_pinned_indexed` misses here; this
/// test pins the distinct `observe_materialize_scope` /
/// `artifact_current_indexed` fallback behavior.
#[test]
fn observe_materialize_scope_refuses_stale_artifact_for_live_scheduler_scope() {
    /// Doctored content hash that no real content produces.
    const STALE_HASH: [u8; 16] = [0xEE; 16];

    let host = VerterHost::new_standalone(HostConfig::default());
    let scope = "/self_root_obs/live_scope_stale_artifact.ts";
    upsert(
        &host,
        scope,
        "export interface Probe { a: number; }\nexport const probe = 1;\n",
    );
    let real_indexed = host
        .ensure_indexed_ready(scope)
        .expect("scope IndexedReady materialises");
    let real_hash = real_indexed.whole_hash;
    assert_ne!(
        real_hash, STALE_HASH,
        "fixture invariant: the real content hash differs from the planted stale hash",
    );

    let ctx: &dyn ResolverContext = &host;
    // Anchor non-vacuity: before planting, the live scope has a
    // tear-free observation rooted on the REAL hash.
    let pre_plant = ctx
        .observe_materialize_scope(scope)
        .expect("fixture invariant: a live scope has a tear-free observation before planting");
    assert_eq!(
        pre_plant.whole_hash(),
        real_hash,
        "fixture invariant: the pre-plant observation pins the real content version",
    );

    // Plant a synthetic STALE `IndexedReady`. `FileArtifactStore::insert`
    // drains the real artifact, so the store now holds ONLY the stale
    // entry while the scheduler still reports `real_hash` — the
    // lingering-stale state. The `DerivedRawState` stays live (the file
    // was never evicted).
    let mut stale = (*real_indexed).clone();
    stale.whole_hash = STALE_HASH;
    host.project_type_store()
        .indexed()
        .insert(Arc::from(scope), Arc::new(stale));

    // Fixture invariants: the live, non-evicted `DerivedRawState` makes
    // the scheduler report the real hash; the scheduler-pinned authority
    // misses (no artifact at `real_hash`); a permissive `get_any` would
    // surface the planted stale artifact.
    assert!(
        host.derived_raw_cache()
            .get(scope)
            .is_some_and(|d| !d.evicted),
        "fixture invariant: the scope has a live (non-evicted) DerivedRawState entry",
    );
    assert_eq!(
        ctx.authoritative_current_content_hash(scope),
        Some(real_hash),
        "fixture invariant: the scheduler reports the real content hash for the live scope",
    );
    assert!(
        host.current_content_pinned_indexed(scope).is_none(),
        "fixture invariant: the scheduler-pinned authority misses — only the STALE_HASH \
         artifact is stored, the pin resolves the real hash",
    );
    assert_eq!(
        host.project_type_store()
            .indexed()
            .get_any(scope)
            .expect("get_any still returns the planted stale entry")
            .whole_hash,
        STALE_HASH,
        "fixture invariant: get_any surfaces the planted stale artifact — the pre-fix \
         artifact-current fallback read shape",
    );

    // The discriminating assertion: `observe_materialize_scope` MUST NOT
    // surface the stale artifact for a live scheduler scope.
    let observation = ctx.observe_materialize_scope(scope);
    assert!(
        observation.is_none(),
        "observe_materialize_scope MUST return None for a live (non-evicted) scheduler \
         scope whose scheduler-pinned authority missed — the artifact-current `get_any` \
         fallback is restricted to canonicals with NO DerivedRawState entry at all. A \
         pre-fix tree takes the `get_any` branch (the DerivedRawState is merely \
         non-evicted) and surfaces the stale artifact here.",
    );
    if let Some(observation) = observation {
        assert_ne!(
            observation.whole_hash(),
            STALE_HASH,
            "even were observe_materialize_scope to return Some, it MUST NOT self-root on \
             the planted stale artifact's doctored whole hash",
        );
    }
}

/// The materialize-memo publish site DEGRADES — never panics — when
/// `observe_materialize_scope` returns `None`.
///
/// Root cause this test guards: the publish site
/// (`meta_resolve/materialize/field_types.rs`) called
/// `observe_materialize_scope(scope).expect(..)`. A `None` observation
/// is a LEGITIMATE outcome (a session tombstone, an evicted/unloaded
/// scope, or no recoverable artifact) — the observation contract is
/// "missing observation ⇒ skip shared-cache admission, still return a
/// value." The `expect` turned that legitimate `None` into a panic
/// *before* materialization.
///
/// ## Fixture — an evicted scope (a `None`-observation scope)
///
/// A real `.ts` file is upserted (live `DerivedRawState` + a
/// `FileArtifactStore` artifact) then evicted. `evict` marks the
/// `DerivedRawState` `evicted` while leaving the `FileArtifactStore`
/// artifact in place. `observe_materialize_scope` returns `None` for
/// such a scope (the scheduler authority refuses — the entry is
/// evicted — AND the artifact-current authority refuses — the surviving
/// artifact is a stale evicted leftover).
///
/// ## Discrimination property
///
/// - PRE-FIX: the publish site's `observe_materialize_scope(scope)
///   .expect("materialize scope must have a real indexed scope
///   identity")` PANICS — the `catch_unwind` below captures `Err`.
/// - POST-FIX: the publish site degrades — it skips the
///   `MaterializeMemoDb` `get_or_compute` admission (no view-correct
///   scope identity to self-root with), lowers under the scope's
///   surviving `shallow_file_state` content version (NOT a fabricated
///   all-zero `NodeScopeId`), and returns the freshly-computed value.
///   The `catch_unwind` captures `Ok`, and no `MaterializeMemoDb` entry
///   is admitted.
#[test]
fn materialize_memo_publish_site_degrades_on_none_scope_observation() {
    use crate::resolver_core::ComponentMetaQueryEngine;
    use crate::semantic_query::ProjectionMode;

    let host = VerterHost::new_standalone(HostConfig::default());
    let scope = "/self_root_obs/publish_site_none_observation.ts";
    upsert(&host, scope, "export interface Probe { a: number; }\n");
    assert!(
        host.ensure_indexed_ready(scope).is_some(),
        "fixture invariant: the scope IndexedReady materialises",
    );

    // Evict the scope: `DerivedRawState` is marked evicted; the
    // `FileArtifactStore` artifact survives as a stale leftover.
    host.evict(scope);

    let ctx: &dyn ResolverContext = &host;
    // Fixture invariant: `observe_materialize_scope` returns `None` for
    // the evicted scope — the exact `None`-observation outcome the
    // publish site must tolerate.
    assert!(
        ctx.observe_materialize_scope(scope).is_none(),
        "fixture invariant: observe_materialize_scope returns None for an evicted scope",
    );

    let db = host.project_type_store().materialize_memo_db();
    let entries_before = db.live_count();

    // Drive the production publish path. Pre-fix the `.expect()` inside
    // `materialize_component_meta_type_expr_until_stable_full` panics on
    // the `None` observation; post-fix it degrades and returns a value.
    let probe_expr = TypeExpr::Ref {
        name: Arc::from("Probe"),
        type_arguments: Arc::from(Vec::<TypeExpr>::new()),
    };
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut engine = ComponentMetaQueryEngine::new(&host);
        crate::meta_resolve::materialize::materialize_component_meta_type_expr_until_stable_full(
            &probe_expr,
            scope,
            ProjectionMode::Navigate,
            &mut engine,
        )
    }));

    // The discriminating assertion: the publish path MUST NOT panic on a
    // `None` observation.
    let materialized = match outcome {
        Ok(materialized) => materialized,
        Err(_) => panic!(
            "the materialize-memo publish site MUST NOT panic on a None scope \
             observation — a None observation (session tombstone, evicted/unloaded \
             scope, no recoverable artifact) is a legitimate outcome and must degrade: \
             skip shared-cache admission, still return the freshly-computed value. A \
             pre-fix `observe_materialize_scope(scope).expect(..)` panics here.",
        ),
    };

    // The degraded path still returns a freshly-computed value.
    assert!(
        matches!(
            &materialized.type_expr,
            TypeExpr::Ref { name, .. } if name.as_ref() == "Probe"
        ) || !matches!(&materialized.type_expr, TypeExpr::Unknown { .. }),
        "the degraded publish path still returns a freshly-computed value to the caller",
    );

    // The degraded path admits NO `MaterializeMemoDb` entry — there is
    // no view-correct scope identity to self-root a shared entry with.
    assert_eq!(
        db.live_count(),
        entries_before,
        "the degraded publish path MUST NOT admit a MaterializeMemoDb entry — a None \
         scope observation has no view-correct scope identity to self-root with, so \
         shared-cache admission is skipped to avoid a mis-rooted entry",
    );
}
