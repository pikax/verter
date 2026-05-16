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
    FactVersionRef, ResolvedDeclarationKind, ResolvedTypeDeclaration, ResolverContext, RouteDemand,
    StoreView,
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

    let _ = db.get_or_compute(&key, ctx, || {
        Some((Some(imported_symbol(c, "stale")), planted_self_root(c)))
    });

    let mut cold_ran = false;
    let warm = db
        .get_or_compute(&key, ctx, || {
            cold_ran = true;
            Some((
                Some(imported_symbol(c, "recomputed")),
                empty_fact_signature(),
            ))
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
                decl_owned.as_str(),
                "Probe",
            );
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
    let scope_owned = scope.to_string();
    let primed_dep_sig = Arc::clone(&dep_sig);
    let primed = db
        .get_or_compute(&key, ctx, move || {
            let sig = engine_fact_signature_for_materialize_memo(
                ctx,
                scope_owned.as_str(),
                &primed_dep_sig,
            );
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

    let owned = c.to_string();
    let primed = db
        .get_or_compute(&key, ctx, || {
            let sig = engine_fact_signature_for_exported_type(ctx, owned.as_str(), "Probe");
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

    let owned = c.to_string();
    let _ = db
        .get_or_compute(&key, ctx, || {
            let sig = engine_fact_signature_for_exported_type(ctx, owned.as_str(), "Probe");
            Some((Some(imported_symbol(c, "stale")), sig))
        })
        .expect("cold publish succeeds");

    skip_drain_sibling_edit(&host, c);

    let ctx2: &dyn ResolverContext = &host;
    let mut cold_ran = false;
    let warm = db
        .get_or_compute(&key, ctx2, || {
            cold_ran = true;
            Some((
                Some(imported_symbol(c, "recomputed")),
                empty_fact_signature(),
            ))
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

    let owned = c.to_string();
    let _ = db
        .get_or_compute(&key, ctx, || {
            let sig = engine_fact_signature_for_exported_type(ctx, owned.as_str(), "Probe");
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

    let owned = c.to_string();
    let _ = db
        .get_or_compute(&key, ctx, || {
            let sig = engine_fact_signature_for_exported_type(ctx, owned.as_str(), "Probe");
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

    let owned = c.to_string();
    let _ = db
        .get_or_compute(&key, ctx, || {
            let sig = engine_fact_signature_for_exported_type(ctx, owned.as_str(), "Probe");
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

    let owned = c.to_string();
    let _ = db
        .get_or_compute(&key, ctx, || {
            let sig = engine_fact_signature_for_exported_type(ctx, owned.as_str(), "Probe");
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

    let owned = c.to_string();
    let _ = db
        .get_or_compute(&key, ctx, || {
            let sig = engine_fact_signature_for_prepared_target(
                ctx,
                owned.as_str(),
                "Probe",
                owned.as_str(),
                "Probe",
            );
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
/// (called by the materialize write-through), which roots the keyed
/// scope canonical via `engine_fact_signature_for_canonical_surface`.
/// An unrelated-sibling body edit to the scope canonical shifts only
/// the self-root `FileWholeHash` — `engine_fact_signature_for_canonical_surface`
/// records `SyntacticExportSet`, which fingerprints the export NAME set
/// (unchanged when an existing `Sibling`'s member body is edited).
///
/// This canary complements
/// [`materialize_memo_db_observed_dependency_edit_rejects_warm_entry`]:
/// that test edits an observed *dependency* and so discriminates the
/// producer's observed-dep merge; this one edits the keyed *scope*
/// canonical and so discriminates the producer's self-root
/// `FileWholeHash`. Verified: neutering `self_root_fact` flips this
/// canary RED.
#[test]
fn materialize_memo_db_self_root_sibling_edit_rejects_warm_entry() {
    use crate::resolver_core::component_meta_query_engine::engine_fact_signature_for_materialize_memo;
    use crate::semantic_query::ProjectionMode;

    let host = VerterHost::new_standalone(HostConfig::default());
    let c = "/self_root_e2e/memo.ts";
    load_tracked_keyed(&host, c);
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

    let owned = c.to_string();
    let _ = db
        .get_or_compute(&key, ctx, || {
            // No observed dependencies — the discriminator is the
            // scope canonical's own self-root `FileWholeHash`.
            let sig = engine_fact_signature_for_materialize_memo(
                ctx,
                owned.as_str(),
                &empty_dep_signature(),
            );
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

    let owned = c.to_string();
    let _ = db
        .get_or_compute(&key, ctx, || {
            let sig =
                engine_fact_signature_for_canonical_member(ctx, owned.as_str(), "Probe", "field");
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

/// `MaterializeMemoDb`'s producer roots every observed canonical by its
/// CURRENT-content `FileWholeHash`, independent of the
/// [`crate::semantic_query::DepVersion`] variant recorded for it.
///
/// Discriminating property: the entry is keyed on `scope`, and its
/// `materialized_dep_signature` lists an observed dependency `dep` with
/// a NON-`WholeHash` `DepVersion` (`DepVersion::RouteGeneration`). The
/// entry is cold-published with the EXACT production producer signature
/// — [`engine_fact_signature_for_materialize_memo`]. The legacy
/// `dep_signature_to_fact_signature` bridge keeps only
/// `DepVersion::WholeHash` entries and would drop a `RouteGeneration`
/// dependency entirely, leaving `dep` unrooted. The producer instead
/// roots `dep` by its current-content `FileWholeHash`. `dep` is then
/// edited through the skip-drain hook, shifting `dep`'s whole hash. A
/// producer that relied on the `WholeHash`-only filter would leave the
/// entry valid (no fact mentions `dep`) and serve it stale; with `dep`
/// rooted by its current whole hash, the warm read misses and
/// recomputes. (`MaterializeMemoDb::invalidate_canonical` matches only
/// the keyed scope, so a normal `upsert(dep)` would not drain this
/// scope-keyed entry regardless — the hook keeps the test isolated
/// from any own-canonical drain side-effect.)
#[test]
fn materialize_memo_db_non_whole_hash_observed_dependency_edit_rejects_warm_entry() {
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

    // The materialization walk observed `dep` via a NON-`WholeHash`
    // dependency — a `RouteGeneration` entry. The legacy
    // `dep_signature_to_fact_signature` bridge would drop this entry;
    // the producer must root `dep` by its current whole hash anyway.
    let dep_sig: crate::semantic_query::DepSignature = Arc::from(vec![(
        Arc::<str>::from(dep),
        DepVersion::RouteGeneration(1),
    )]);
    let scope_owned = scope.to_string();
    let primed_dep_sig = Arc::clone(&dep_sig);
    let primed = db
        .get_or_compute(&key, ctx, move || {
            let sig = engine_fact_signature_for_materialize_memo(
                ctx,
                scope_owned.as_str(),
                &primed_dep_sig,
            );
            // The signature MUST mention `dep` — a `RouteGeneration`
            // observed canonical is still rooted by its whole hash.
            assert!(
                sig.iter().any(|f| matches!(
                    f,
                    crate::resolver_core::FactVersionRef::FileWholeHash { canonical_id, .. }
                        if canonical_id == dep
                )),
                "producer must root the RouteGeneration-observed canonical {dep} by its \
                 current-content FileWholeHash — the WholeHash-only bridge would drop it",
            );
            Some((materialized("stale", Arc::clone(&primed_dep_sig)), sig))
        })
        .expect("cold publish succeeds");
    assert!(
        matches!(&primed.type_expr, TypeExpr::Unknown { raw } if raw == "stale"),
        "fixture invariant: cold publish stores the primed materialized expression",
    );

    // Edit ONLY the observed dependency through the skip-drain hook.
    upsert_skip_drain(
        &host,
        dep,
        "export interface Helper { a: string; b: number; }\n",
    );
    assert!(
        host.ensure_indexed_ready(dep).is_some(),
        "dep IndexedReady re-materialises",
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
        "MaterializeMemoDb warm read MUST reject the entry after a content edit to a \
         canonical the materialization walk observed via a NON-WholeHash DepVersion — \
         the producer roots every observed canonical by its current-content \
         FileWholeHash regardless of DepVersion variant. A producer that relied on the \
         WholeHash-only dep_signature_to_fact_signature filter would leave the entry \
         valid and serve stale.",
    );
    assert!(
        matches!(&warm.type_expr, TypeExpr::Unknown { raw } if raw == "recomputed"),
        "the rejected warm entry must not bubble its stale materialized expression",
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
    let sig = engine_fact_signature_for_materialize_memo(ctx, scope, &dep_sig);

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
