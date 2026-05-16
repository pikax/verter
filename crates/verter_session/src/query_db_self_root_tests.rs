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
/// The dependency file is then edited through the skip-own-drain hook
/// (so neither the scope's nor the dependency's own-canonical drain
/// removes the memo entry), shifting `dep`'s whole hash. A producer
/// helper that recorded only the scope self-root would leave the entry
/// valid and serve stale; with the observed-dep fact recorded, the
/// warm read misses and recomputes. Reverting the helper to drop the
/// observed-dep merge flips this test.
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
    // hook so neither the scope's nor the dependency's own-canonical
    // drain removes the memo entry.
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
