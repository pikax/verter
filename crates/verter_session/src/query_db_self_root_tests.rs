//! Self-version-root discriminator tests for the component-meta
//! query-identity caches AND the structural carriers
//! (`MaterializeStructureDb`, `RefCycleResultDb`).
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
//! The two secondary-canonical tests at the end cover the producer
//! widening for `PreparedTargetDb` (the declaring canonical is a
//! second self-root) and `MaterializeMemoDb` (every canonical observed
//! during materialization is a dependency fact): they prime a warm
//! entry, edit a *secondary* (non-keyed-scope) canonical through the
//! production [`crate::VerterHost::upsert`] — which performs no
//! own-canonical drain, so the entry physically survives — and assert
//! the warm read misses. A producer that recorded no fact for the
//! secondary canonical would validate the entry stale.

use std::collections::BTreeSet;
use std::sync::Arc;

use verter_semantic::analysis::type_solver::query_engine::ProjectedMember;
use verter_type_expr::TypeExpr;

use crate::fact_signature_helpers::empty_fact_signature;
use crate::project_semantic_dispatch::raise::MaterializedTypeExpr;
use crate::resolver_core::cache_keys::PreparedTargetCacheKey;
use crate::resolver_core::component_meta_query_engine::ResolvedImportedRegistrySymbol;
use crate::resolver_core::{
    FactVersionRef, MaterializeScopeObservation, ResolvedDeclarationKind, ResolvedTypeDeclaration,
    ResolverContext, RouteDemand, StoreView,
};
use crate::semantic_query::ProjectionMode;
use crate::{HostConfig, UpsertRequest, VerterHost};

/// A self-root `FileWholeHash` byte pattern for a planted (untracked)
/// entry. Distinct from any real content hash.
const PLANTED_HASH: [u8; 16] = [0xAB; 16];

/// Upsert through the production [`VerterHost::upsert`] path. The
/// upsert performs no own-canonical query-identity cache drain, so a
/// warm entry for the upserted canonical physically survives and the
/// test can observe whether its self-root validation detects the edit.
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

/// The self-root canonical set for a [`planted_self_root`] signature —
/// the single keyed canonical it roots. `PreparedTargetDb` entries
/// carry an explicit `self_root_canonicals` set (the cache key omits
/// the routed declaring canonical); a synthetic prime passes this so
/// the entry's strict-validation set matches the planted fact.
fn planted_self_root_canonicals(canonical: &str) -> Arc<[Arc<str>]> {
    Arc::from(vec![Arc::<str>::from(canonical)])
}

/// An empty self-root canonical set — for a synthetic recompute whose
/// `empty_fact_signature` carries no self-root fact.
fn empty_self_root_canonicals() -> Arc<[Arc<str>]> {
    Arc::from(Vec::<Arc<str>>::new())
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
                validated_at_generation: ctx.project_type_store().current_project_generation(),
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
                    validated_at_generation: ctx.project_type_store().current_project_generation(),
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
            planted_self_root_canonicals(scope),
        ))
    });

    let mut cold_ran = false;
    let warm = db
        .get_or_compute(&key, ctx, || {
            cold_ran = true;
            Some((
                Some((Arc::<str>::from(scope), Arc::<str>::from("recomputed"))),
                empty_fact_signature(),
                empty_self_root_canonicals(),
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
/// is then edited through the production `upsert` (which performs no
/// own-canonical drain, so the entry physically survives), shifting its
/// whole hash. A producer helper that rooted only the active scope would
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
            // No re-export hop: the declaring canonical IS the final
            // routed canonical, so `routed_decl` is `None`.
            let sig = engine_fact_signature_for_prepared_target(
                ctx,
                scope_owned.as_str(),
                "Probe",
                observed_scope_hash,
                decl_owned.as_str(),
                "Probe",
                observed_decl_hash,
                None,
            )
            .expect("provenance-pure signature builds — both observed artifacts present");
            Some((
                Some((Arc::<str>::from(decl_canonical), Arc::<str>::from("stale"))),
                sig,
                Arc::from(vec![
                    Arc::<str>::from(active_scope),
                    Arc::<str>::from(decl_canonical),
                ]),
            ))
        })
        .expect("cold publish succeeds — both keyed canonicals tracked");
    assert_eq!(
        primed.map(|(_, n)| n.as_ref().to_string()),
        Some("stale".to_string()),
        "fixture invariant: cold publish stores the primed target",
    );

    // Edit ONLY the declaring file. The upsert performs no
    // own-canonical drain, so the cache entry physically survives.
    upsert(
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
                empty_self_root_canonicals(),
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
        cache_suppress: false,
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
    let db = host.project_type_store().shape_cache_db();
    let probe_expr = Arc::new(TypeExpr::Ref {
        name: Arc::from("Probe"),
        type_arguments: Arc::from(Vec::<TypeExpr>::new()),
    });
    let key = crate::component_meta_caches::ShapeCacheKey::type_expr_whole(
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
        "ShapeCacheDb (TypeExpr subject) MUST NOT serve a warm entry whose self-root \
         names an untracked keyed canonical",
    );
    assert!(
        matches!(&warm.type_expr, TypeExpr::Unknown { raw } if raw == "recomputed"),
        "the rejected entry must not bubble its stale materialized expression",
    );
}

/// Universal `ShapeCacheDb` (SemanticNode
/// subject) validates the keyed scope canonical's self-root strictly.
///
/// Discriminating property: the prime attempt's materialized
/// expression carries the marker `"stale"`; the recompute carries
/// `"recomputed"`. A lazy validator admits the stale expression and
/// the second `get_or_compute` returns it; the strict validator
/// rejects admission for an untracked keyed canonical.
///
/// Mirrors `materialize_memo_db_untracked_self_root_rejects_warm_entry`
/// exactly — both subjects share the
/// `cooperative_get_or_insert_with_post_publish` + fact-signature
/// self-root contract.
#[test]
fn member_shape_cache_db_untracked_self_root_rejects_warm_entry() {
    use crate::semantic_query::{ProjectionMode, SemanticNodeId};

    let host = host_with_unrelated_file();
    let c = "/self_root_qdb/member_shape_never_loaded.ts";
    assert_untracked(&host, c);
    let ctx: &dyn ResolverContext = &host;
    let db = host.project_type_store().shape_cache_db();
    // Use a synthetic SemanticNodeId — the test exercises the cache's
    // self-root validation contract, not the production graph.
    let key = crate::component_meta_caches::ShapeCacheKey::semantic_node_whole(
        Arc::<str>::from(c),
        SemanticNodeId(7),
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
        "ShapeCacheDb (SemanticNode subject) MUST NOT serve a warm entry whose \
         self-root names an untracked keyed canonical",
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
/// The dependency file is then edited through the production `upsert`,
/// shifting `dep`'s whole hash. A producer helper that recorded only
/// the scope self-root would leave the entry valid and serve stale;
/// with the observed-dep fact recorded, the warm read misses and
/// recomputes. Reverting the helper to drop the observed-dep merge
/// flips this test.
///
/// The entry is keyed on `scope`, not `dep`: an `upsert(dep)` would
/// never match this scope-keyed entry, and the production `upsert`
/// performs no own-canonical drain in any case — so the entry's
/// survival across the dependency edit is guaranteed and the warm-read
/// rejection is driven purely by the observed-dep `FileWholeHash` fact.
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
    let db = host.project_type_store().shape_cache_db();
    let probe_expr = Arc::new(TypeExpr::Ref {
        name: Arc::from("Probe"),
        type_arguments: Arc::from(Vec::<TypeExpr>::new()),
    });
    let key = crate::component_meta_caches::ShapeCacheKey::type_expr_whole(
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

    // Edit ONLY the observed dependency. The entry is keyed on the
    // scope, not `dep`, and the production `upsert` performs no
    // own-canonical drain — so the scope-keyed entry physically
    // survives and the warm-read rejection is driven purely by the
    // observed-dep `FileWholeHash` fact (see the docstring).
    upsert(
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
// End-to-end producer-level self-root canaries.
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
//  3. Edits the keyed canonical through the production
//     [`crate::VerterHost::upsert`] — which performs no own-canonical
//     drain, so the entry physically survives — with an
//     **unrelated-sibling-body edit**: a member of the `Sibling`
//     declaration changes type while `Probe` and the file's export name
//     set are left untouched.
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
// validator of each of the nine query-identity caches end-to-end.
// `FileArtifactStore` (a content-addressed cache, NOT one of the nine
// query-identity caches) is content-pinned independently; these nine
// producer-level canaries prove the self-version-root wiring of the
// query-identity layer end-to-end.

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
/// production `upsert` (which performs no own-canonical drain, so the
/// entry physically survives) and re-`ensure_indexed_ready` it. `Probe`
/// and the file's export name set are untouched — only the whole-file
/// hash shifts.
fn sibling_body_edit(host: &VerterHost, canonical: &str) {
    upsert(host, canonical, &keyed_source_with_sibling("string"));
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
/// then edited through the production `upsert` with an
/// unrelated-sibling body edit. The producer signature's parse facts (`Export(Probe)`,
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

    sibling_body_edit(&host, c);

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
                    validated_at_generation: ctx.project_type_store().current_project_generation(),
                },
            )
        })
        .expect("cold publish succeeds");

    sibling_body_edit(&host, c);

    let ctx2: &dyn ResolverContext = &host;
    let mut cold_ran = false;
    let warm = db
        .get_or_compute_admit(&key, ctx2, || {
            cold_ran = true;
            crate::cooperative_admission::ComputeAdmission::Cacheable(
                crate::component_meta_caches::ImportedRegistryEntry {
                    value: Some(Arc::new(imported_symbol(c, "recomputed"))),
                    fact_dep_signature: empty_fact_signature(),
                    validated_at_generation: ctx2.project_type_store().current_project_generation(),
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

    sibling_body_edit(&host, c);

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

    sibling_body_edit(&host, c);

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
            // Active scope and declaring canonical are the same file
            // and there is no re-export hop — `routed_decl` is `None`.
            let sig = engine_fact_signature_for_prepared_target(
                ctx,
                owned.as_str(),
                "Probe",
                observed_keyed_hash,
                owned.as_str(),
                "Probe",
                observed_keyed_hash,
                None,
            )
            .expect("provenance-pure signature builds — observed artifact present");
            Some((
                Some((Arc::<str>::from(c), Arc::<str>::from("stale"))),
                sig,
                planted_self_root_canonicals(c),
            ))
        })
        .expect("cold publish succeeds");

    sibling_body_edit(&host, c);

    let ctx2: &dyn ResolverContext = &host;
    let mut cold_ran = false;
    let warm = db
        .get_or_compute(&key, ctx2, || {
            cold_ran = true;
            Some((
                Some((Arc::<str>::from(c), Arc::<str>::from("recomputed"))),
                empty_fact_signature(),
                empty_self_root_canonicals(),
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

/// `PreparedTargetDb::invalidate_canonical` must scan the entry's
/// `self_root_canonicals` — a routed third declaring file is a
/// load-bearing dependency the cache key never encodes.
///
/// A `PreparedTarget` entry whose requested name re-exports through an
/// intermediate module to a third file carries that final routed
/// declaring canonical in `self_root_canonicals`, while the cache key
/// encodes only `(active_scope, decl_canonical)`. Editing the third
/// file invalidates the entry's content dependency, so
/// `invalidate_canonical(third_file)` MUST remove it.
///
/// Discriminating fixture: an entry is cold-published with
/// `self_root_canonicals = [active_scope, decl_canonical, third_file]`
/// — `third_file` is absent from the key. The entry's
/// `fact_dep_signature` is empty, so it never goes stale on its own:
/// the ONLY thing that can remove it is `invalidate_canonical`.
///
/// - **Pre-fix tree:** `invalidate_canonical`'s `filter_map` matches
///   only `active_scope_canonical_id` / `decl_canonical_id`.
///   `invalidate_canonical(third_file)` matches nothing, the entry
///   survives, and `live_count()` stays `1` — this test FAILS.
/// - **Post-fix tree:** the scan also matches an entry whose
///   `self_root_canonicals` contains the canonical, so the entry is
///   removed and `live_count()` is `0`.
#[test]
fn prepared_target_db_invalidate_canonical_scans_routed_self_root() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let active_scope = "/self_root_qdb/ptgt_inv_scope.ts";
    let decl_canonical = "/self_root_qdb/ptgt_inv_decl.ts";
    // The THIRD declaring file: the requested name's final routed
    // declaration target, reached through a re-export hop. It is NOT in
    // the `PreparedTargetCacheKey`.
    let third_file = "/self_root_qdb/ptgt_inv_routed_decl.ts";
    upsert(
        &host,
        active_scope,
        "import { Probe } from './ptgt_inv_decl';\nexport type ReExport = Probe;\n",
    );
    upsert(
        &host,
        decl_canonical,
        "export { Probe } from './ptgt_inv_routed_decl';\n",
    );
    upsert(&host, third_file, "export interface Probe { a: number; }\n");
    host.ensure_indexed_ready(active_scope)
        .expect("scope indexed");
    host.ensure_indexed_ready(decl_canonical)
        .expect("decl indexed");
    host.ensure_indexed_ready(third_file)
        .expect("routed decl indexed");

    let ctx: &dyn ResolverContext = &host;
    let db = host.project_type_store().prepared_target_db();
    let key = prepared_target_key(active_scope, decl_canonical);

    // Cold-publish. The entry's `self_root_canonicals` names the THIRD
    // declaring file (the routed target) alongside the two keyed
    // canonicals. The signature is empty so the entry never self-
    // invalidates — only `invalidate_canonical` can remove it.
    let primed = db
        .get_or_compute(&key, ctx, || {
            Some((
                Some((Arc::<str>::from(third_file), Arc::<str>::from("Probe"))),
                empty_fact_signature(),
                Arc::from(vec![
                    Arc::<str>::from(active_scope),
                    Arc::<str>::from(decl_canonical),
                    Arc::<str>::from(third_file),
                ]),
            ))
        })
        .expect("cold publish succeeds");
    assert!(
        primed.is_some(),
        "fixture invariant: the cold publish stores the prepared target",
    );
    assert_eq!(
        db.live_count(),
        1,
        "fixture invariant: exactly one PreparedTargetDb entry is live after the cold publish",
    );

    // Invalidate the THIRD declaring file — a file absent from the
    // cache key but present in the entry's `self_root_canonicals`.
    db.invalidate_canonical(third_file);

    assert_eq!(
        db.live_count(),
        0,
        "PreparedTargetDb::invalidate_canonical(third_file) MUST remove an entry whose \
         `self_root_canonicals` names `third_file` — a routed declaring file is a \
         load-bearing dependency the cache key never encodes. A pre-fix scan that \
         matches only the keyed canonicals leaves the stale entry in the map.",
    );
    assert!(
        db.peek(&key, ctx).is_none(),
        "the entry rooted on the edited routed declaring file MUST NOT survive as a \
         warm peek hit after `invalidate_canonical(third_file)`",
    );
}

/// `PreparedTargetDb::live_counter` must not drift above the live entry
/// count when a stale entry is evicted on the warm-read path.
///
/// `cooperative_get_or_insert` removes a stale already-published entry
/// on its own warm-hit reject path, but with no removal hook — the
/// publish-side `live_counter` increment (in the `compute` closure) is
/// then unbalanced. `get_or_compute` resolves the warm hit itself so
/// this Db owns the matching decrement.
///
/// Discriminating fixture: a fresh `PreparedTargetDb` with its OWN
/// counter (isolated from the shared `component_meta_cache_live`
/// aggregate). An entry is cold-published with the real producer
/// signature ([`engine_fact_signature_for_prepared_target`]) so it
/// validates at publish time and IS admitted. The keyed file is then
/// edited through the production `upsert` — which performs no
/// own-canonical drain, so the entry lingers stale. A second
/// `get_or_compute` triggers the warm-read eviction; its compute returns
/// `None` (no republish). The live counter must then equal
/// `entries.len()`.
///
/// - **Pre-fix tree:** the helper's warm-reject `map.remove` drops the
///   entry from the map (`entries.len() == 0`) but does NOT decrement
///   `live_counter` (it stays `1`) — `live_counter != entries.len()`,
///   so this test FAILS.
/// - **Post-fix tree:** `get_or_compute` resolves the stale warm hit
///   itself, removing the entry WITH a `fetch_sub`, so `live_counter`
///   tracks `entries.len()` exactly.
#[test]
fn prepared_target_db_live_counter_does_not_drift_on_stale_eviction() {
    use crate::component_meta_caches::PreparedTargetDb;
    use crate::resolver_core::component_meta_query_engine::engine_fact_signature_for_prepared_target;
    use std::sync::atomic::{AtomicU64, Ordering};

    let host = VerterHost::new_standalone(HostConfig::default());
    let keyed = "/self_root_qdb/ptgt_drift_keyed.ts";
    upsert(&host, keyed, "export interface Probe { a: number; }\n");
    host.ensure_indexed_ready(keyed).expect("keyed indexed");
    let ctx: &dyn ResolverContext = &host;

    // A fresh Db with its OWN counter, isolated from the shared
    // `component_meta_cache_live` aggregate so the assertion observes
    // exactly this Db's publish/evict bookkeeping.
    let counter = Arc::new(AtomicU64::new(0));
    let db = PreparedTargetDb::with_counter(Arc::clone(&counter));
    let key = prepared_target_key(keyed, keyed);

    // Cold-publish with the EXACT production producer signature, rooted
    // on the keyed file's CURRENT observed hash — it validates at
    // publish time, so the entry IS admitted.
    let observed_keyed_hash = observed_whole_hash(ctx, keyed);
    let owned = keyed.to_string();
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
                None,
            )
            .expect("provenance-pure signature builds — observed artifact present");
            Some((
                Some((Arc::<str>::from(keyed), Arc::<str>::from("Probe"))),
                sig,
                planted_self_root_canonicals(keyed),
            ))
        })
        .expect("cold publish succeeds — keyed canonical tracked");
    assert_eq!(
        counter.load(Ordering::Relaxed),
        1,
        "fixture invariant: the cold publish incremented the live counter to 1",
    );
    assert_eq!(
        db.live_count(),
        1,
        "fixture invariant: exactly one entry is live after the cold publish",
    );

    // Edit the keyed file. The upsert performs no own-canonical drain,
    // so the entry lingers stale, to be evicted lazily on the next warm
    // read.
    upsert(
        &host,
        keyed,
        "export interface Probe { a: number; b: string; }\n",
    );

    // Second call: the warm-read path observes the now-stale entry and
    // evicts it. The compute returns `None` (no republish) so the only
    // counter movement is the stale eviction.
    let ctx2: &dyn ResolverContext = &host;
    let mut cold_ran = false;
    let _ = db.get_or_compute(&key, ctx2, || {
        cold_ran = true;
        None
    });
    assert!(
        cold_ran,
        "fixture invariant: the stale warm entry was rejected, so the cold path ran",
    );

    assert_eq!(
        counter.load(Ordering::Relaxed),
        db.live_count() as u64,
        "PreparedTargetDb::live_counter MUST equal the live entry count after a stale \
         entry is evicted on the warm-read path. A pre-fix tree leaves the counter at 1 \
         while the map is empty — the warm-reject removal decremented nothing.",
    );
    assert_eq!(
        counter.load(Ordering::Relaxed),
        0,
        "after the stale entry is evicted and the recompute republishes nothing, the \
         live counter must be 0",
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
    let db = host.project_type_store().shape_cache_db();
    let probe_expr = Arc::new(TypeExpr::Ref {
        name: Arc::from("Probe"),
        type_arguments: Arc::from(Vec::<TypeExpr>::new()),
    });
    let key = crate::component_meta_caches::ShapeCacheKey::type_expr_whole(
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

    sibling_body_edit(&host, c);

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

/// `MaterializeMemoDb`'s producer REFUSES shared-memo admission for an
/// entry whose materialisation walk observed a dependency via
/// `DepVersion::RouteGeneration`.
///
/// Route generation is not a real production-validating fact: there is
/// no authoritative route-generation counter, no production emitter,
/// and `StoreView::validates_fact_signature` has no `RouteGeneration`
/// arm — a `RouteGeneration`-only observation is unrooted.
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
    let db = host.project_type_store().shape_cache_db();
    let probe_expr = Arc::new(TypeExpr::Ref {
        name: Arc::from("Probe"),
        type_arguments: Arc::from(Vec::<TypeExpr>::new()),
    });
    let key = crate::component_meta_caches::ShapeCacheKey::type_expr_whole(
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
    let db = host.project_type_store().shape_cache_db();
    let probe_expr = Arc::new(TypeExpr::Ref {
        name: Arc::from("Probe"),
        type_arguments: Arc::from(Vec::<TypeExpr>::new()),
    });
    let key = crate::component_meta_caches::ShapeCacheKey::type_expr_whole(
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
    // hash shifts to H2. `MaterializeMemoDb` keys on the scope, not
    // `dep`, and the production `upsert` performs no own-canonical
    // drain in any case, so no entry eviction perturbs this test.
    upsert(
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
/// re-read the scope's current content (a publish-race defect: the
/// scope hash read twice non-atomically — once for the value, once
/// for the signature) would emit `H_current`; the stale value would
/// then be published rooted by a fresh-looking current hash and
/// validate on every warm read, permanently masking an edit landing in
/// the materialise -> write-through race window.
///
/// RED proof (this fix changed the builder signature): with the body
/// reverted to re-read the scope's current content hash for the
/// self-root (the publish-race defect), the emitted scope `FileWholeHash`
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
    // shifts. The production `upsert` performs no own-canonical drain
    // (irrelevant here — no entry is published — so no eviction can
    // perturb the fixture).
    upsert(&host, scope, "export type Probe = string;\n");
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
         hash — re-reading the current hash is the publish-race defect.",
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
    let db = host.project_type_store().shape_cache_db();
    let probe_expr = Arc::new(TypeExpr::Ref {
        name: Arc::from("Probe"),
        type_arguments: Arc::from(Vec::<TypeExpr>::new()),
    });
    let key = crate::component_meta_caches::ShapeCacheKey::type_expr_whole(
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
    // window the publish-race defect describes.
    upsert(&host, scope, "export type Probe = string;\n");
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
// Discrimination shape (deterministic, no artifact-survival dependency — a
// same-canonical upsert replaces the canonical's parse-fact registry with
// the new content's, so a stale observed hash resolves no parse facts
// regardless of whether a stale `FileArtifactStore` artifact lingers):
//
//  1. Load the keyed canonical at content version `H1`; observe `H1`.
//  2. Anchor non-vacuity: the producer signature builder called with
//     `observed_hash = H1` while current == `H1` returns `Some` rooted on
//     `H1`.
//  3. Edit the keyed canonical so current becomes a different `H2`
//     (the prior `H1` parse-fact registry is replaced).
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
    upsert(&host, c, &keyed_source_with_sibling("string"));
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

    upsert(&host, c, &keyed_source_with_sibling("string"));
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

    upsert(&host, c, &keyed_source_with_sibling("string"));
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

    upsert(&host, c, &keyed_source_with_sibling("string"));
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
        None,
    )
    .expect("observed-current signature builds");
    assert!(
        signature_roots_whole_hash(&anchored, c, observed_h1),
        "anchor: the signature for the observed-current case must root on H1",
    );

    upsert(&host, c, &keyed_source_with_sibling("string"));
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
            None,
        )
        .is_none(),
        "PreparedTargetDb's signature builder MUST return None for the STALE observed \
         hash H1 after the keyed canonical was edited to H2. A pre-fix builder re-reads \
         current content for both self-roots and returns Some rooted on H2.",
    );

    let current_sig = engine_fact_signature_for_prepared_target(
        ctx2, c, "Probe", current_h2, c, "Probe", current_h2, None,
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
// Cache coverage. The producer test below covers the `ResolvabilityDb`
// query cache, whose producer is externally callable, whose cold
// value-compute is sourced from the prepared-decl bundle (view-isolated),
// AND whose producer admits a torn/base-rooted entry pre-fix so the leak
// is observable. A second test pins `observed_prepared_type_decl` itself —
// the single-artifact observation point shared by the `OwnerCollectionDb`
// producer. The remaining caches' producers are not amenable to a
// producer-level overlay test: `DeclarationLookupDb` and the
// imported-registry resolver recover their value through shallow-metadata
// / dispatch reads that consult the non-content-pinned
// `FileArtifactStore::get_any`, which itself returns the overlay candidate
// to a base recompute (a separate pre-existing content-pinning gap — see
// this file's earlier `[debt]` note), so a producer-level test cannot
// isolate the self-root fix; `OwnerCollectionDb`'s producer refuses
// admission of the torn entry pre-fix (see the note above the
// `ResolvabilityDb` test); `PreparedTargetDb`'s producer is `pub(super)`
// and not reachable from this module. Their producer-side hash-source
// (base-only `shallow_file_state` → view-aware
// `authoritative_current_content_hash`) is covered by the
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

    host.materialize_overlay_indexed_ready_with_view(canonical, &view)
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
    assert!(
        view.overlay_content_hash_for(canonical).is_some(),
        "fixture invariant: the overlay covers the canonical",
    );
    host.materialize_overlay_indexed_ready_with_view(canonical, &view)
        .expect("overlay IndexedReady materialises");

    let overlay_store_view = host
        .resolver_store_view()
        .with_session_overlay(&host, &view);
    let overlay_ctx = SessionResolverContext::new(
        &host,
        &view,
        &overlay_store_view,
        std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new()),
    );
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

    let overlay_store_view = host
        .resolver_store_view()
        .with_session_overlay(&host, &view);
    let overlay_ctx = SessionResolverContext::new(
        &host,
        &view,
        &overlay_store_view,
        std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new()),
    );
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
    let overlay_store_view = host
        .resolver_store_view()
        .with_session_overlay(&host, &view);
    let overlay_ctx = SessionResolverContext::new(
        &host,
        &view,
        &overlay_store_view,
        std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new()),
    );
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
    let db = host.project_type_store().shape_cache_db();
    let probe_expr = Arc::new(TypeExpr::Ref {
        name: Arc::from("Probe"),
        type_arguments: Arc::from(Vec::<TypeExpr>::new()),
    });
    let key = crate::component_meta_caches::ShapeCacheKey::type_expr_whole(
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

    let db = host.project_type_store().shape_cache_db();
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

// ---------------------------------------------------------------------------
// Cooperative-admission joiner view-validation — cache-level discriminator.
// ---------------------------------------------------------------------------

/// Cache-level cross-view discriminator through `ImportedRegistryDb`.
///
/// `ImportedRegistryDb::get_or_compute_admit` routes through the
/// `cooperative_admit_with_post_publish` substrate. The substrate's
/// single-flight coalesces concurrent cold misses for the same key —
/// but two requests carrying the same key can run under different
/// views (a base context and a session/overlay context). Their
/// resolved-import results are NOT interchangeable: each must validate
/// the entry's `fact_dep_signature` against its OWN content identity,
/// exactly as a warm hit does.
///
/// Setup: a real file is upserted, giving it a base content hash. The
/// winner runs `get_or_compute_admit` under the base host context and
/// publishes a `Cacheable` entry whose `fact_dep_signature` self-roots
/// the keyed canonical at its BASE hash — so the winner's own
/// `revalidate_after_compute` (base view) accepts it and the entry
/// lands in the map. The follower runs the SAME key under a
/// `SessionResolverContext` whose overlay re-roots the keyed canonical
/// to a DIFFERENT content hash. The follower coalesces onto the
/// winner's flight, wakes onto the published entry, and runs
/// `ImportedRegistryDb`'s `validate` closure against its OWN overlay
/// view.
///
/// Deterministic rendezvous: the winner is held inside its `compute()`
/// closure; the test driver releases it ONLY after polling the
/// winner's `InflightSlot` strong count to PROVE the follower has
/// coalesced onto that slot (count `>= 4` — see the loop below). A
/// fixed sleep would not discriminate the joiner path: on a slow
/// worker the winner could publish first and the follower would then
/// pass via the warm-map-reject path instead of the joiner-fork path.
///
/// Discrimination:
/// - Pre-fix: the cooperative joiner ran `project` (NOT `validate`) on
///   the winner's published entry — no view check — so the follower
///   inherited the winner's base-rooted symbol and its own cold
///   closure never ran (`follower_cold_ran == false`).
/// - Post-fix: the joiner runs `ImportedRegistryDb`'s `validate`
///   closure; the base-rooted self-root mismatches the follower's
///   overlay hash, `validate` returns `None`, the follower forks and
///   cold-recomputes for its own view (`follower_cold_ran == true`),
///   returning its OWN symbol.
#[test]
fn imported_registry_cooperative_joiner_validates_against_follower_view() {
    use crate::resolver_core::SessionResolverContext;
    use crate::session_view::{OverlaidView, SessionView};
    use rustc_hash::FxHashMap;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc;
    use std::thread;
    use std::time::{Duration, Instant};

    let canonical = "/coop_xview/imported.ts";
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert(
        &host,
        canonical,
        "export interface Probe { base: number; }\n",
    );
    let base_hash = host
        .ensure_indexed_ready(canonical)
        .expect("base IndexedReady materialises")
        .whole_hash;
    let host = Arc::new(host);

    // The winner's published entry self-roots the keyed canonical at
    // its BASE content hash — so the winner's own
    // `revalidate_after_compute` (run under the base view) accepts the
    // entry and it is admitted into the map.
    let base_self_root: Arc<[FactVersionRef]> = Arc::from(vec![FactVersionRef::FileWholeHash {
        canonical_id: canonical.to_string(),
        hash: base_hash,
    }]);

    let db_handle = Arc::clone(&host);
    let key: (Arc<str>, Arc<str>) = (Arc::<str>::from(canonical), Arc::<str>::from("Probe"));

    let (tx_winner_in_compute, rx_winner_in_compute) = mpsc::channel::<()>();
    let (tx_release_winner, rx_release_winner) = mpsc::channel::<()>();

    // Winner thread — runs under the base host context.
    let winner_host = Arc::clone(&db_handle);
    let winner_key = key.clone();
    let winner_self_root = Arc::clone(&base_self_root);
    let winner = thread::spawn(move || {
        let ctx: &dyn ResolverContext = winner_host.as_ref();
        let db = winner_host.project_type_store().imported_registry_db();
        db.get_or_compute_admit(&winner_key, ctx, || {
            tx_winner_in_compute.send(()).expect("winner: signal claim");
            rx_release_winner
                .recv()
                .expect("winner: released by driver");
            crate::cooperative_admission::ComputeAdmission::Cacheable(
                crate::component_meta_caches::ImportedRegistryEntry {
                    value: Some(Arc::new(imported_symbol(canonical, "winner-base"))),
                    fact_dep_signature: Arc::clone(&winner_self_root),
                    validated_at_generation: ctx.project_type_store().current_project_generation(),
                },
            )
        })
    });

    rx_winner_in_compute
        .recv()
        .expect("winner entered compute (claimed the inflight slot)");

    // Follower thread — runs under a session whose overlay re-roots the
    // keyed canonical to a DIFFERENT content hash, so the winner's
    // base-rooted entry must not validate under the follower's view.
    let follower_cold_ran = Arc::new(AtomicBool::new(false));
    let follower_host = Arc::clone(&db_handle);
    let follower_key = key.clone();
    let follower_cold_flag = Arc::clone(&follower_cold_ran);
    let follower = thread::spawn(move || {
        // Overlay re-roots the keyed canonical: a different source body
        // yields a different overlay content hash.
        let overlay_source: Arc<str> = Arc::from("export interface Probe { overlaid: string; }\n");
        let mut overlays: FxHashMap<String, Arc<str>> = FxHashMap::default();
        overlays.insert(canonical.to_string(), Arc::clone(&overlay_source));
        let view = OverlaidView::new(Arc::clone(&follower_host), overlays);
        let overlay_hash = view
            .overlay_content_hash_for(canonical)
            .expect("overlay content hash present");
        assert_ne!(
            overlay_hash, base_hash,
            "fixture invariant: the overlay hash must differ from the base hash",
        );
        follower_host
            .materialize_overlay_indexed_ready_with_view(canonical, &view)
            .expect("overlay IndexedReady materialises");
        let session_store_view = follower_host
            .resolver_store_view()
            .with_session_overlay(&follower_host, &view);
        let session_ctx = SessionResolverContext::new(
            &follower_host,
            &view,
            &session_store_view,
            std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new()),
        );
        let db = follower_host.project_type_store().imported_registry_db();
        db.get_or_compute_admit(&follower_key, &session_ctx, || {
            follower_cold_flag.store(true, Ordering::SeqCst);
            crate::cooperative_admission::ComputeAdmission::Cacheable(
                crate::component_meta_caches::ImportedRegistryEntry {
                    value: Some(Arc::new(imported_symbol(canonical, "follower-overlay"))),
                    fact_dep_signature: Arc::from(vec![FactVersionRef::FileWholeHash {
                        canonical_id: canonical.to_string(),
                        hash: overlay_hash,
                    }]),
                    validated_at_generation: session_ctx
                        .project_type_store()
                        .current_project_generation(),
                },
            )
        })
    });

    // Deterministic rendezvous — prove the follower has coalesced onto
    // the winner's in-flight slot BEFORE releasing the winner. A fixed
    // sleep proves nothing: on a slow worker the winner would publish
    // first, the follower would then reject the warm map entry and
    // recompute, and the test would pass via the warm-map-reject path
    // instead of the joiner-fork path it advertises.
    //
    // Poll the winner's `InflightSlot` strong count. While the winner
    // is parked inside its `compute()` closure (blocked on
    // `rx_release_winner`), the substrate holds exactly three `Arc`s on
    // the slot: the in-flight table entry, the winner's `slot` local,
    // and the winner's `panic_guard.slot`. The follower bumps the count
    // to 4 the instant it clones its own `Arc` via the slot-acquisition
    // `table.entry(key).or_insert_with(...).clone()` — past which it
    // deterministically reaches the cooperative joiner wait branch. We
    // release the winner only once the count is `>= 4`, so the follower
    // is a PROVEN joiner on every run regardless of worker speed.
    let db = host.project_type_store().imported_registry_db();
    let rendezvous_deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if db
            .inflight_table_for_test()
            .slot_strong_count(&key)
            .is_some_and(|count| count >= 4)
        {
            break;
        }
        assert!(
            Instant::now() < rendezvous_deadline,
            "follower failed to coalesce onto the winner's in-flight \
             slot within 10s — the deterministic joiner rendezvous is \
             broken",
        );
        std::hint::spin_loop();
    }
    tx_release_winner.send(()).expect("release winner");

    let winner_result = winner.join().expect("winner joined");
    let follower_result = follower.join().expect("follower joined");

    // The winner ran under the base view and resolves its own symbol.
    assert_eq!(
        winner_result.flatten().map(|s| s.exported_name.clone()),
        Some("winner-base".to_string()),
        "the winner resolves its own base-view symbol",
    );

    // Discriminator: pre-fix the cooperative joiner ran `project` on the
    // winner's published entry with no view check, so the follower
    // never ran its own cold closure. Post-fix the joiner runs
    // `ImportedRegistryDb`'s `validate` closure; the winner's
    // base-rooted self-root mismatches the follower's overlay hash, so
    // `validate` returns `None` and the follower forks.
    assert!(
        follower_cold_ran.load(Ordering::SeqCst),
        "ImportedRegistryDb's cooperative joiner MUST run the cache's \
         `validate` closure against the follower's OWN overlay view — \
         the winner's entry self-roots the keyed canonical at the BASE \
         hash and must not validate under the follower's overlay view, \
         so the follower MUST fork and cold-recompute. Pre-fix the \
         joiner ran `project` (no view check) and inherited the winner's \
         base-rooted symbol without recomputing.",
    );
    assert_eq!(
        follower_result.flatten().map(|s| s.exported_name.clone()),
        Some("follower-overlay".to_string()),
        "the follower's resolved symbol MUST be its OWN overlay-view \
         recompute — the winner's base-view symbol is not interchangeable \
         across views",
    );
}

/// P2 removal-cleanup discriminator through `ImportedRegistryDb`.
///
/// `ImportedRegistryDb` does publish-side bookkeeping: its
/// cooperative-admission `post_publish` bumps the shared
/// `component_meta_cache_live` counter and registers the key in the
/// per-canonical reverse index. A cross-view joiner-fork removes the
/// winner's published entry — and that removal MUST run the cache's
/// removal-side cleanup (decrement the counter, drop the reverse-index
/// registration) symmetrically with `post_publish`. A raw `DashMap`
/// removal skips the cleanup.
///
/// This test drives a full joiner-fork (winner publishes a base-rooted
/// entry; follower under an overlay rejects it, forks, and publishes
/// its own overlay-rooted entry) and then checks that
/// `ImportedRegistryDb`'s contribution to the shared live counter
/// equals its actual live entry count.
///
/// Discrimination — counter consistency:
/// - Pre-fix: when the joiner-fork's entry removal is a raw
///   `map.remove_if` that skips the removal cleanup, the winner's
///   `post_publish` increments the counter (+1); the joiner-fork
///   removes the winner entry with NO decrement; the follower's
///   re-publish increments again (+1). The counter delta is +2 while
///   the map holds exactly ONE entry — over-counted by one.
/// - Post-fix: the joiner-fork removal routes through
///   `removal_cleanup`, decrementing the counter (−1). The counter
///   delta is +1, matching the one live entry.
///
/// The counter is the discriminating signal: pre-fix `counter_delta`
/// (+2) ≠ `entries_delta` (+1); post-fix they are equal. The
/// reverse-index assertion below is an additional consistency check —
/// for this same-key fork the follower re-publishes the same key so
/// the reverse index ends consistent on both trees; it guards against
/// a regression that would leave the key unregistered.
#[test]
fn imported_registry_joiner_fork_removal_keeps_live_counter_consistent() {
    use crate::resolver_core::SessionResolverContext;
    use crate::session_view::{OverlaidView, SessionView};
    use rustc_hash::FxHashMap;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc;
    use std::thread;
    use std::time::{Duration, Instant};

    let canonical = "/coop_xview_p2/imported.ts";
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert(
        &host,
        canonical,
        "export interface Probe { base: number; }\n",
    );
    let base_hash = host
        .ensure_indexed_ready(canonical)
        .expect("base IndexedReady materialises")
        .whole_hash;
    let host = Arc::new(host);

    let base_self_root: Arc<[FactVersionRef]> = Arc::from(vec![FactVersionRef::FileWholeHash {
        canonical_id: canonical.to_string(),
        hash: base_hash,
    }]);

    let key: (Arc<str>, Arc<str>) = (Arc::<str>::from(canonical), Arc::<str>::from("Probe"));

    // Snapshot the shared live counter AND `ImportedRegistryDb`'s entry
    // count BEFORE the cooperative race. The shared counter is bumped
    // by every component-meta cache, so the discriminator is the
    // DELTA, not the absolute value.
    let live_counter = Arc::clone(&host.project_type_store().counters.component_meta_cache_live);
    let counter_before = live_counter.load(Ordering::Relaxed);
    let entries_before = host
        .project_type_store()
        .imported_registry_db()
        .live_count();

    let (tx_winner_in_compute, rx_winner_in_compute) = mpsc::channel::<()>();
    let (tx_release_winner, rx_release_winner) = mpsc::channel::<()>();

    // Winner — publishes a base-rooted `Cacheable` entry.
    let winner_host = Arc::clone(&host);
    let winner_key = key.clone();
    let winner_self_root = Arc::clone(&base_self_root);
    let winner = thread::spawn(move || {
        let ctx: &dyn ResolverContext = winner_host.as_ref();
        let db = winner_host.project_type_store().imported_registry_db();
        db.get_or_compute_admit(&winner_key, ctx, || {
            tx_winner_in_compute.send(()).expect("winner: signal claim");
            rx_release_winner
                .recv()
                .expect("winner: released by driver");
            crate::cooperative_admission::ComputeAdmission::Cacheable(
                crate::component_meta_caches::ImportedRegistryEntry {
                    value: Some(Arc::new(imported_symbol(canonical, "winner-base"))),
                    fact_dep_signature: Arc::clone(&winner_self_root),
                    validated_at_generation: ctx.project_type_store().current_project_generation(),
                },
            )
        })
    });

    rx_winner_in_compute
        .recv()
        .expect("winner entered compute (claimed the inflight slot)");

    // Follower — under an overlay that re-roots the keyed canonical, so
    // the winner's base-rooted entry fails the follower's view check;
    // the follower forks and cold-recomputes its own entry.
    let follower_cold_ran = Arc::new(AtomicBool::new(false));
    let follower_host = Arc::clone(&host);
    let follower_key = key.clone();
    let follower_cold_flag = Arc::clone(&follower_cold_ran);
    let follower = thread::spawn(move || {
        let overlay_source: Arc<str> = Arc::from("export interface Probe { overlaid: string; }\n");
        let mut overlays: FxHashMap<String, Arc<str>> = FxHashMap::default();
        overlays.insert(canonical.to_string(), Arc::clone(&overlay_source));
        let view = OverlaidView::new(Arc::clone(&follower_host), overlays);
        let overlay_hash = view
            .overlay_content_hash_for(canonical)
            .expect("overlay content hash present");
        assert_ne!(
            overlay_hash, base_hash,
            "fixture invariant: the overlay hash must differ from the base hash",
        );
        follower_host
            .materialize_overlay_indexed_ready_with_view(canonical, &view)
            .expect("overlay IndexedReady materialises");
        let session_store_view = follower_host
            .resolver_store_view()
            .with_session_overlay(&follower_host, &view);
        let session_ctx = SessionResolverContext::new(
            &follower_host,
            &view,
            &session_store_view,
            std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new()),
        );
        let db = follower_host.project_type_store().imported_registry_db();
        db.get_or_compute_admit(&follower_key, &session_ctx, || {
            follower_cold_flag.store(true, Ordering::SeqCst);
            crate::cooperative_admission::ComputeAdmission::Cacheable(
                crate::component_meta_caches::ImportedRegistryEntry {
                    value: Some(Arc::new(imported_symbol(canonical, "follower-overlay"))),
                    fact_dep_signature: Arc::from(vec![FactVersionRef::FileWholeHash {
                        canonical_id: canonical.to_string(),
                        hash: overlay_hash,
                    }]),
                    validated_at_generation: session_ctx
                        .project_type_store()
                        .current_project_generation(),
                },
            )
        })
    });

    // Deterministic rendezvous — release the winner only once the
    // follower has PROVABLY coalesced onto the winner's in-flight slot
    // (strong count `>= 4`: table + winner.slot + winner.panic_guard +
    // follower.slot). See
    // `imported_registry_cooperative_joiner_validates_against_follower_view`.
    let rendezvous_db = host.project_type_store().imported_registry_db();
    let rendezvous_deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if rendezvous_db
            .inflight_table_for_test()
            .slot_strong_count(&key)
            .is_some_and(|count| count >= 4)
        {
            break;
        }
        assert!(
            Instant::now() < rendezvous_deadline,
            "follower failed to coalesce onto the winner's in-flight \
             slot within 10s — the deterministic joiner rendezvous is \
             broken",
        );
        std::hint::spin_loop();
    }
    tx_release_winner.send(()).expect("release winner");

    let _winner_result = winner.join().expect("winner joined");
    let _follower_result = follower.join().expect("follower joined");

    // The follower MUST have forked — this test only discriminates the
    // removal cleanup if the joiner-fork path actually ran.
    assert!(
        follower_cold_ran.load(Ordering::SeqCst),
        "fixture invariant: the follower MUST fork and cold-recompute \
         (the winner's base-rooted entry must fail the follower's \
         overlay-view check) — otherwise the joiner-fork removal under \
         test never executed",
    );

    let counter_after = live_counter.load(Ordering::Relaxed);
    let db = host.project_type_store().imported_registry_db();
    let entries_after = db.live_count();
    let counter_delta: u64 = counter_after - counter_before;
    let entries_delta: u64 = (entries_after - entries_before) as u64;

    // After a joiner-fork the map holds exactly ONE entry for the key
    // (the follower's overlay-rooted re-publish; the winner's
    // base-rooted entry was removed by the fork).
    assert_eq!(
        entries_delta, 1,
        "after the joiner-fork the cache must hold exactly one live \
         entry for the key — the winner's entry removed, the follower's \
         re-published",
    );

    // Discriminator: pre-fix the joiner-fork's raw removal skipped the
    // counter decrement, so the shared counter over-counts (delta +2)
    // while the map holds one entry. Post-fix the removal routes
    // through `removal_cleanup` and the counter delta matches the live
    // entry count.
    assert_eq!(
        counter_delta, entries_delta,
        "the shared `component_meta_cache_live` counter delta ({counter_delta}) \
         MUST equal `ImportedRegistryDb`'s live-entry delta ({entries_delta}) \
         after a joiner-fork. A larger counter delta means the \
         joiner-fork's entry removal skipped the cache-owned removal \
         cleanup — the winner's entry was removed without decrementing \
         the counter that its `post_publish` incremented.",
    );

    // Reverse-index consistency: the one live key must be registered
    // exactly once; no removal must have left it unregistered.
    assert!(
        db.reverse_index_contains_for_test(&key),
        "the live key must be registered in the per-canonical reverse \
         index — the joiner-fork's removal cleanup must not unregister \
         a key that a subsequent re-publish re-registered",
    );
}

/// P2 — the removal-cleanup reverse-index `unregister` must be
/// identity-checked: a stale cleanup must NOT delete a FRESH
/// registration a concurrent cold-publish put under the same key.
///
/// `ImportedRegistryDb`'s cooperative-admission `removal_cleanup`
/// closure drops the removed entry's per-canonical reverse-index
/// registration. The substrate's `map.remove_if` (which removes the
/// caller's stale entry) and that `removal_cleanup` are NOT atomic:
/// a caller preempted between them gives a concurrent caller a window
/// to cold-publish a FRESH entry under the same key and `register` it
/// in the reverse index. If the `removal_cleanup`'s `unregister` were
/// key-only, the resumed stale cleanup would delete that fresh
/// registration — leaving a live entry in `entries` that
/// `invalidate_canonical` (which drains via the reverse index) can no
/// longer find. A later content edit then leaves that entry stale and
/// served.
///
/// This test drives that exact interleaving deterministically and
/// single-threaded. The winning caller publishes entry A under `key`,
/// self-rooting the canonical at its BASE content hash. A second read
/// then runs under a `SessionResolverContext` whose overlay re-roots
/// the canonical to a DIFFERENT content hash — so the warm hit on A
/// fails the overlay-view `validate` and the substrate removes A.
/// (An overlay is session-local: it does NOT run the host's
/// `invalidate_canonical` cascade, so entry A genuinely survives in
/// the base `entries` map between the two reads.)
///
/// The substrate's test-only `REMOVAL_CLEANUP_PRE_HOOK` — fired AFTER
/// `map.remove_if` removed A but BEFORE `removal_cleanup` runs — is
/// installed to cold-publish a FRESH, DISTINCT entry B under the SAME
/// `key` (the work a concurrent cold-publisher does while the removing
/// caller is preempted). The second read's `compute` returns `Failed`
/// so nothing publishes AFTER the removal — B is left as the sole live
/// entry under `key`.
///
/// Discrimination — the hook IS the synchronisation point, no timing
/// sleep:
/// - Pre-fix (`12e29bcbf`): `CanonicalReverseIndex::unregister` is
///   key-only. A's stale cleanup deletes the `key` registration even
///   though it now belongs to B. B's entry stays in `entries` but is
///   orphaned from the reverse index → `reverse_index_contains` is
///   `false` and a subsequent `invalidate_canonical` MISSES B,
///   leaving it stale-cached.
/// - Post-fix: `unregister` is `EntryIdentity`-checked. The stored
///   registration now names B (`EntryIdentity::of(entry_B)`), which
///   does not match A's identity, so A's cleanup is a no-op. B's
///   registration survives → `reverse_index_contains` is `true` and
///   `invalidate_canonical` evicts B.
#[test]
fn imported_registry_removal_cleanup_preserves_fresh_reverse_index_registration() {
    use crate::resolver_core::SessionResolverContext;
    use crate::session_view::{OverlaidView, SessionView};
    use rustc_hash::FxHashMap;
    use std::sync::atomic::{AtomicBool, Ordering};

    let canonical = "/coop_xview_p2_identity/imported.ts";
    let host = VerterHost::new_standalone(HostConfig::default());
    // Base content — entry A is published self-rooting this hash.
    upsert(
        &host,
        canonical,
        "export interface Probe { base: number; }\n",
    );
    let base_hash = host
        .ensure_indexed_ready(canonical)
        .expect("base IndexedReady materialises")
        .whole_hash;
    let host = Arc::new(host);

    let key: (Arc<str>, Arc<str>) = (Arc::<str>::from(canonical), Arc::<str>::from("Probe"));

    // Publish entry A under `key`, self-rooting the canonical at its
    // BASE hash so A's own post-compute revalidation (base view)
    // accepts it and it lands in the map + reverse index.
    {
        let ctx: &dyn ResolverContext = host.as_ref();
        let db = host.project_type_store().imported_registry_db();
        let base_self_root: Arc<[FactVersionRef]> =
            Arc::from(vec![FactVersionRef::FileWholeHash {
                canonical_id: canonical.to_string(),
                hash: base_hash,
            }]);
        let published = db
            .get_or_compute_admit(&key, ctx, || {
                crate::cooperative_admission::ComputeAdmission::Cacheable(
                    crate::component_meta_caches::ImportedRegistryEntry {
                        value: Some(Arc::new(imported_symbol(canonical, "entry-A"))),
                        fact_dep_signature: Arc::clone(&base_self_root),
                        validated_at_generation: ctx
                            .project_type_store()
                            .current_project_generation(),
                    },
                )
            })
            .expect("entry A publishes under the base view");
        assert_eq!(
            published.map(|s| s.exported_name.clone()),
            Some("entry-A".to_string()),
            "fixture invariant: entry A is the freshly published value",
        );
    }
    assert!(
        host.project_type_store()
            .imported_registry_db()
            .reverse_index_contains_for_test(&key),
        "fixture invariant: entry A's publish registered `key` in the \
         per-canonical reverse index",
    );

    // An overlay that re-roots the keyed canonical to a DIFFERENT
    // content hash: a different source body yields a different overlay
    // content hash, so entry A's base-rooted self-root fails the
    // overlay-view `validate` and the warm read removes A.
    let overlay_source: Arc<str> = Arc::from("export interface Probe { overlaid: string; }\n");
    let mut overlays: FxHashMap<String, Arc<str>> = FxHashMap::default();
    overlays.insert(canonical.to_string(), Arc::clone(&overlay_source));
    let view = OverlaidView::new(Arc::clone(&host), overlays);
    let overlay_hash = view
        .overlay_content_hash_for(canonical)
        .expect("overlay content hash present");
    assert_ne!(
        overlay_hash, base_hash,
        "fixture invariant: the overlay hash must differ from the base hash",
    );
    host.materialize_overlay_indexed_ready_with_view(canonical, &view)
        .expect("overlay IndexedReady materialises");

    // The hook fires INSIDE `remove_published_entry_with_cleanup`,
    // AFTER `map.remove_if` removed entry A but BEFORE `removal_cleanup`
    // runs — the exact window a concurrent cold-publisher would use. It
    // cold-publishes a FRESH, DISTINCT entry B under the SAME `key`,
    // self-rooting the canonical at the OVERLAY hash (so B is valid for
    // the overlay view). A latch makes the publish fire exactly once
    // even though the substrate's hook is `Fn`.
    let hook_host = Arc::clone(&host);
    let hook_key = key.clone();
    let fired = Arc::new(AtomicBool::new(false));
    let hook_fired = Arc::clone(&fired);
    let _hook_guard =
        crate::cooperative_admission::install_removal_cleanup_pre_hook(Box::new(move || {
            if hook_fired.swap(true, Ordering::SeqCst) {
                return;
            }
            let db = hook_host.project_type_store().imported_registry_db();
            let fresh_entry = Arc::new(crate::component_meta_caches::ImportedRegistryEntry {
                value: Some(Arc::new(imported_symbol(canonical, "entry-B-fresh"))),
                fact_dep_signature: Arc::from(vec![FactVersionRef::FileWholeHash {
                    canonical_id: canonical.to_string(),
                    hash: overlay_hash,
                }]),
                validated_at_generation: hook_host
                    .project_type_store()
                    .current_project_generation(),
            });
            // Cold-publish B: inserts into `entries` AND registers in
            // the reverse index with B's own `EntryIdentity` — exactly
            // what a concurrent cold winner's `post_publish` does.
            db.insert_for_test(hook_key.clone(), fresh_entry);
        }));

    // Warm-hit `get_or_compute_admit` under the overlay session view:
    //   warm hit on A → `validate` rejects A (base self-root fails the
    //     overlay view) → `remove_published_entry_with_cleanup` removes A
    //   → REMOVAL_CLEANUP_PRE_HOOK fires → B is cold-published
    //   → the production `removal_cleanup` runs `unregister(identity_A)`.
    // `compute` returns `Failed` so nothing publishes after the
    // removal — B is the sole live entry under `key`.
    {
        let session_store_view = host
            .resolver_store_view()
            .with_session_overlay(&host, &view);
        let session_ctx = SessionResolverContext::new(
            &host,
            &view,
            &session_store_view,
            std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new()),
        );
        let db = host.project_type_store().imported_registry_db();
        let outcome = db.get_or_compute_admit(&key, &session_ctx, || {
            crate::cooperative_admission::ComputeAdmission::Failed
        });
        assert!(
            outcome.is_none(),
            "fixture invariant: the warm read rejected stale entry A and \
             `compute` returned `Failed`, so the call yields `None`",
        );
    }

    assert!(
        fired.load(Ordering::SeqCst),
        "fixture invariant: the removal-cleanup pre-hook MUST have fired — \
         otherwise the racey cold-publish under test never executed",
    );

    let db = host.project_type_store().imported_registry_db();

    // entry B must still be the live entry under `key`.
    assert_eq!(
        db.live_count(),
        1,
        "fixture invariant: entry B is the sole live entry under the key \
         (A removed by the warm-hit reject, B cold-published by the hook)",
    );

    // DISCRIMINATOR: B's reverse-index registration must survive A's
    // stale removal cleanup. Pre-fix the key-only `unregister` deletes
    // it; post-fix the identity-checked `unregister` is a no-op for a
    // registration that now names B.
    assert!(
        db.reverse_index_contains_for_test(&key),
        "the removal-cleanup `unregister` MUST be identity-checked: a \
         stale cleanup for the REMOVED entry A must not delete the FRESH \
         reverse-index registration a concurrent cold-publish created \
         for entry B. Pre-fix the key-only `unregister` deleted B's \
         registration, orphaning a live entry from `invalidate_canonical`.",
    );

    // CONSEQUENCE DISCRIMINATOR: the orphaned-registration bug's real
    // damage — `invalidate_canonical` drains via the reverse index, so
    // a lost registration means a later content edit leaves entry B
    // stale-cached. With the registration preserved, `invalidate_canonical`
    // finds and evicts B.
    db.invalidate_canonical(canonical);
    assert_eq!(
        db.live_count(),
        0,
        "after the identity-checked removal cleanup preserved entry B's \
         reverse-index registration, `invalidate_canonical` MUST find and \
         evict B. Pre-fix B's registration was deleted, so \
         `invalidate_canonical` drained an empty bucket and left B \
         stale-cached.",
    );
}

// ===========================================================================
// Structural carriers — `MaterializeStructureDb` and `RefCycleResultDb`.
//
// These two query-identity caches carry an explicit `self_root_canonicals`
// set: `MaterializeStructureDb` roots ONLY the `base` node's
// declaration-origin file (the consumer materialise scope is NOT a
// self-root — R7 cross-owner reuse); `RefCycleResultDb` roots the BFS root
// file plus every visited declaration's file. Every warm read validates
// those self-roots strictly via `ReadSetSignature::validate_with_self_roots`,
// so a same-canonical / visited-canonical content edit — or an untracked
// self-root canonical — rejects the entry. The tests below discriminate the
// strict warm-read validator against the lax `validate(ctx)`, and confirm
// that a content edit to the base origin / root / a visited canonical
// rejects the warm entry through the production `upsert` — same-canonical
// invalidation is lazy, the carrier rejects the edit on its own self-root
// rather than relying on an eager upsert-time drain. The strict-validator
// mechanism itself is exercised by a planted-self-root entry below.
// ===========================================================================

/// Intern a stable scope-less `Object` node — a `base` whose identity
/// does not depend on any file's content (so the cache key is stable
/// across a scope edit and the only shifting fact is the scope's
/// `FileWholeHash`).
fn intern_global_object(host: &VerterHost) -> crate::semantic_query::SemanticNodeId {
    use crate::semantic_query::{SemanticNodeData, SurfaceView};
    host.project_type_store()
        .semantic_graph()
        .intern_node(SemanticNodeData::Object(SurfaceView {
            members: Arc::from(Vec::new().into_boxed_slice()),
            call_signatures: Arc::from(Vec::new().into_boxed_slice()),
            construct_signatures: Arc::from(Vec::new().into_boxed_slice()),
            index_signatures: Arc::from(Vec::new().into_boxed_slice()),
            keyspace: None,
            has_index_signature: false,
        }))
}

/// `MaterializeStructureDb::peek` runs its self-root canonicals
/// through the **strict** whole-hash validator, not the lax one.
///
/// This exercises the strict-validator MECHANISM in isolation: the
/// production materialiser never roots the consumer materialise scope
/// (it self-roots ONLY the `base` node's declaration-origin file — R7
/// cross-owner reuse), so to drive the strict path deterministically a
/// synthetic entry is PLANTED with a `FileWholeHash` self-root for an
/// untracked canonical and `self_root_canonicals = [canonical]`.
///
/// Discriminating property: the lax `validate(ctx)` routes the
/// untracked `FileWholeHash` through `StoreView::validates`, whose
/// untracked-accept arm returns `true` — a lax validator serves the
/// planted entry warm. The strict `validate_with_self_roots` routes a
/// listed self-root through `validates_self_root_whole_hash`, which
/// rejects an untracked self-root. The peek misses iff the validator
/// is strict; reverting `MaterializeStructureDb::peek` to
/// `validate(ctx)` flips this test.
#[test]
fn materialize_structure_db_planted_untracked_self_root_rejects_warm_entry() {
    use crate::component_meta_caches::MaterializeStructureEntry;
    use crate::component_meta_materialize::{
        MaterializationScope, MaterializeOutcome, MaterializeStructureCacheKey,
    };
    use crate::semantic_query::ProjectionMode;

    let host = host_with_unrelated_file();
    let scope = "/struct_carrier_qdb/ms_never_loaded.ts";
    assert_untracked(&host, scope);
    let ctx: &dyn ResolverContext = &host;
    let db = host.project_type_store().materialize_structure_db();

    let base = intern_global_object(&host);
    let key = MaterializeStructureCacheKey {
        scope_canonical_id: Arc::from(scope),
        base,
        scope_axis: MaterializationScope::TopLevel,
        mode: ProjectionMode::Expanded,
    };

    // Plant a synthetic entry: the carrier's facts rail holds a
    // self-root `FileWholeHash` for the untracked scope, and
    // `self_root_canonicals` lists it. A lax validator admits this; the
    // strict one does not.
    let facts: Arc<[FactVersionRef]> = Arc::from(vec![FactVersionRef::FileWholeHash {
        canonical_id: scope.to_string(),
        hash: PLANTED_HASH,
    }]);
    let planted = Arc::new(MaterializeStructureEntry {
        outcome: MaterializeOutcome::Value(base),
        read_set_signature: crate::fact_signature_helpers::ReadSetSignature::new(facts),
        dispatch_dep_signature: std::sync::Arc::from(Vec::new()),
        self_root_canonicals: planted_self_root_canonicals(scope),
        admission_seq: crate::bounded_query_retention::next_retention_seq(),
        // Live project generation — this test exercises the carrier's
        // strict self-root rejection, not the generation gate, so the
        // stamp must match the live generation.
        validated_at_generation: ctx.project_type_store().current_project_generation(),
    });
    db.entries().insert(key.clone(), planted);

    assert!(
        db.peek(&key, ctx).is_none(),
        "MaterializeStructureDb::peek MUST reject a warm entry whose self-root \
         FileWholeHash names an UNTRACKED scope canonical — the lax `validate` accepts \
         the untracked self-root and serves the entry stale; only the strict \
         `validate_with_self_roots` rejects it.",
    );
}

/// Intern a `base` `Object` node whose origin scope is a
/// `NodeScopeId::File` for `canonical` at `canonical`'s CURRENT observed
/// whole hash — the file-derived-base shape the materialiser roots via
/// `base_node_origin_self_root`. The `base` identity is stable across an
/// edit to `canonical` (the node id and the cache key never shift); the
/// only thing that shifts is the file's `whole_hash` the entry's
/// `base_origin_self_root` `FileWholeHash` records.
fn intern_file_derived_object(
    host: &VerterHost,
    canonical: &str,
) -> crate::semantic_query::SemanticNodeId {
    use crate::semantic_query::{NodeScopeId, SemanticNodeData, SurfaceView};
    let whole_hash = host
        .shallow_file_state(canonical)
        .map(|s| s.whole_hash)
        .expect("file-derived base fixture: canonical must be tracked with a whole hash");
    host.project_type_store()
        .semantic_graph()
        .intern_node_with_scope(
            SemanticNodeData::Object(SurfaceView {
                members: Arc::from(Vec::new().into_boxed_slice()),
                call_signatures: Arc::from(Vec::new().into_boxed_slice()),
                construct_signatures: Arc::from(Vec::new().into_boxed_slice()),
                index_signatures: Arc::from(Vec::new().into_boxed_slice()),
                keyspace: None,
                has_index_signature: false,
            }),
            NodeScopeId::File {
                canonical_id: Arc::from(canonical),
                whole_hash,
                local_scope: None,
            },
        )
}

/// `MaterializeStructureDb` rejects a warm entry after a content edit
/// to the `base` node's declaration-origin file — end-to-end through
/// the production materialiser.
///
/// Same-canonical invalidation is lazy: the upsert performs no eager
/// own-canonical cache drain, so a `MaterializeStructureDb` entry for
/// the edited file physically survives the upsert and must reject the
/// edit on its own self-root. The load-bearing dependency a
/// `MaterializeStructureDb` value carries is the `base` node's
/// `NodeScopeId::File` origin — recorded by the producer as a strict
/// `base_origin_self_root`. This test interns a `base` whose origin
/// scope is `NodeScopeId::File { canonical_id: edited_file }`,
/// materialises + admits, then edits `edited_file` through the
/// production `upsert` and asserts the warm `peek` misses. The `base`
/// node id (hence the cache key) is STABLE across the edit — the only
/// shifting fact is the `base` origin's `FileWholeHash`. (The
/// materialise *scope* is NOT a strict self-root: it is non-load-bearing
/// — see `..._unread_scope_edit_keeps_warm_entry`.)
#[test]
fn materialize_structure_db_base_origin_edit_rejects_warm_entry() {
    use crate::component_meta_materialize::{MaterializationScope, MaterializeStructureCacheKey};
    use crate::semantic_query::ProjectionMode;

    let host = VerterHost::new_standalone(HostConfig::default());
    let edited_file = "/struct_carrier_qdb/ms_base_origin.ts";
    upsert(&host, edited_file, "export type Probe = { a: number };\n");
    assert!(
        host.ensure_indexed_ready(edited_file).is_some(),
        "edited_file IndexedReady materialises",
    );

    let dispatch = host.semantic_dispatch();
    // `base` origin scope is `NodeScopeId::File { edited_file }` at its
    // current whole hash — the file-derived-base self-root.
    let base = intern_file_derived_object(&host, edited_file);
    let key = MaterializeStructureCacheKey {
        scope_canonical_id: Arc::from(edited_file),
        base,
        scope_axis: MaterializationScope::TopLevel,
        mode: ProjectionMode::Expanded,
    };

    // Cold build — publishes a warm entry self-rooted on the `base`
    // node's declaration-origin file.
    let _ = dispatch.materialize_surface(key.clone());
    let db = host.project_type_store().materialize_structure_db();
    assert!(
        db.peek(&key, &host).is_some(),
        "fixture invariant: the cold build admitted a warm MaterializeStructureDb entry \
         self-rooted on the `base` node's declaration-origin file",
    );

    // Edit the `base` origin file through the production `upsert`. The
    // upsert performs no eager own-canonical drain, so the entry
    // physically survives and the strict self-root must reject it.
    upsert(
        &host,
        edited_file,
        "export type Probe = { a: string; b: number };\n",
    );
    assert!(
        host.ensure_indexed_ready(edited_file).is_some(),
        "edited_file IndexedReady re-materialises after the edit",
    );

    assert!(
        db.peek(&key, &host).is_none(),
        "MaterializeStructureDb::peek MUST reject the warm entry after a content edit to \
         the `base` node's declaration-origin file — the entry's strict \
         `base_origin_self_root` catches the shifted FileWholeHash. Same-canonical \
         invalidation is lazy: with no eager upsert-time drain, the carrier rejects an \
         edit to a file the materialisation depends on, on its own self-root.",
    );
}

/// `MaterializeStructureDb` does NOT over-root on the non-load-bearing
/// consumer materialise scope: an entry whose materialisation does not
/// depend on a given consumer scope MUST survive a content edit to that
/// scope (the warm `peek` still hits).
///
/// Discriminating property: the `base` is a `Global`-scoped `Object`
/// (no `NodeScopeId::File` origin → no `base_origin_self_root`) with an
/// empty surface (no traced facts). The consumer materialise scope is a
/// SEPARATE file the materialisation never reads. The cache key excludes
/// `scope_canonical_id` (R7 cross-owner reuse), so the cache key is
/// stable across the scope edit.
///
/// - **Pre-fix tree:** `materialize_structure_read_set` seeds the
///   consumer scope into `self_root_hashes` as a strict self-root
///   `FileWholeHash` and pushes the scope's `SyntacticExportSet` parse
///   fact. A content edit to the scope shifts that `FileWholeHash`, so
///   strict `validate_with_self_roots` REJECTS the entry — the warm
///   `peek` misses.
/// - **Post-fix tree:** the consumer scope is not a self-root and
///   contributes no fact. A `Global` base with no traced facts admits as
///   a zero-self-root, zero-fact entry; a scope edit leaves the entry's
///   signature untouched, so the warm `peek` still HITS.
///
/// This test FAILS against the pre-fix tree (the artificial scope
/// self-root invalidates the warm entry) and PASSES post-fix. It is the
/// direct discriminator for the P1 over-rooting removal.
#[test]
fn materialize_structure_db_unread_scope_edit_keeps_warm_entry() {
    use crate::component_meta_materialize::{MaterializationScope, MaterializeStructureCacheKey};
    use crate::semantic_query::ProjectionMode;

    let host = VerterHost::new_standalone(HostConfig::default());
    let scope = "/struct_carrier_qdb/ms_unread_scope.ts";
    upsert(&host, scope, "export const anchor = 1;\n");
    assert!(
        host.ensure_indexed_ready(scope).is_some(),
        "scope IndexedReady materialises",
    );

    let dispatch = host.semantic_dispatch();
    // `base` is a Global-scoped empty Object — no `NodeScopeId::File`
    // origin (no `base_origin_self_root`) and no members (no traced
    // facts). The materialisation is genuinely content-invariant and
    // does NOT depend on the consumer scope.
    let base = intern_global_object(&host);
    let key = MaterializeStructureCacheKey {
        scope_canonical_id: Arc::from(scope),
        base,
        scope_axis: MaterializationScope::TopLevel,
        mode: ProjectionMode::Expanded,
    };

    // Cold build — publishes a warm entry. Post-fix this entry carries
    // no self-root and no fact (zero-self-root, zero-fact admission).
    let _ = dispatch.materialize_surface(key.clone());
    let db = host.project_type_store().materialize_structure_db();
    assert!(
        db.peek(&key, &host).is_some(),
        "fixture invariant: the cold build admitted a warm MaterializeStructureDb entry",
    );

    // Edit the consumer scope through the production `upsert`. The
    // upsert performs no eager own-canonical drain, so the entry
    // physically survives — and the materialisation does not depend on
    // the scope's content, so the warm `peek` must still hit.
    upsert(
        &host,
        scope,
        "export const anchor = 2;\nexport const extra = 3;\n",
    );
    assert!(
        host.ensure_indexed_ready(scope).is_some(),
        "scope IndexedReady re-materialises after the edit",
    );

    assert!(
        db.peek(&key, &host).is_some(),
        "MaterializeStructureDb::peek MUST still serve the warm entry after a content \
         edit to a consumer materialise scope the materialisation does NOT depend on — \
         the consumer scope is non-load-bearing and is NOT a strict self-root. A tree \
         that seeds the consumer scope as a strict self-root over-roots the entry and \
         invalidates it on an edit the cached value never depended on, breaking R7 \
         cross-owner reuse.",
    );
}

/// Build a `DeclIdentity` carrying `canonical`'s CURRENT observed whole
/// hash — the identity shape the BFS root + visited identities use.
fn ref_cycle_decl_identity(
    host: &VerterHost,
    canonical: &str,
    name: &str,
) -> crate::semantic_query::DeclIdentity {
    let whole_hash = host
        .shallow_file_state(canonical)
        .map(|s| s.whole_hash)
        .unwrap_or([0u8; 16]);
    crate::semantic_query::DeclIdentity {
        canonical_id: Arc::from(canonical),
        whole_hash,
        decl_name: Arc::from(name),
    }
}

/// `RefCycleResultDb` rejects a warm entry after a content edit to its
/// BFS ROOT canonical.
///
/// Same-canonical invalidation is lazy: the upsert performs no eager
/// own-canonical cache drain, so a `RefCycleResultDb` entry for the
/// edited root physically survives the upsert and must reject the edit
/// on its own. The BFS records the root identity's `(canonical,
/// whole_hash)` as a strict self-root; this test edits the root file
/// through the production `upsert` and asserts the warm `peek` misses.
/// The BFS cache key is the `DeclIdentity`, which embeds `whole_hash` —
/// the test holds the identity at its ORIGINAL hash (the `DeclIdentity`
/// a stale caller would still hold); `peek` on that key finds the entry
/// and MUST reject it via the strict self-root.
#[test]
fn ref_cycle_db_root_edit_rejects_warm_entry() {
    use crate::meta_resolve::ref_root_reaches_transitive_cycle_node;

    let host = VerterHost::new_standalone(HostConfig::default());
    let root = "/struct_carrier_qdb/rc_root.ts";
    upsert(&host, root, "export type Probe = { a: number };\n");
    assert!(
        host.ensure_indexed_ready(root).is_some(),
        "root IndexedReady materialises",
    );

    let ctx: &dyn ResolverContext = &host;
    let id = ref_cycle_decl_identity(&host, root, "Probe");

    // Cold BFS — publishes a warm entry self-rooted on `root`.
    let mut fence = Vec::new();
    let _ = ref_root_reaches_transitive_cycle_node(&id, ctx, &mut fence);
    let db = host.project_type_store().ref_cycle_db();
    assert!(
        crate::component_meta_caches::ref_cycle_db_peek(db, &id, ctx).is_some(),
        "fixture invariant: the cold BFS admitted a warm RefCycleResultDb entry \
         self-rooted on the root canonical",
    );

    // Edit the root file through the production `upsert`. The upsert
    // performs no eager own-canonical drain, so the entry physically
    // survives and the strict self-root must reject it.
    upsert(
        &host,
        root,
        "export type Probe = { a: string; b: number };\n",
    );
    assert!(
        host.ensure_indexed_ready(root).is_some(),
        "root IndexedReady re-materialises after the edit",
    );

    let ctx2: &dyn ResolverContext = &host;
    assert!(
        crate::component_meta_caches::ref_cycle_db_peek(db, &id, ctx2).is_none(),
        "RefCycleResultDb::peek MUST reject the warm entry after a content edit to its \
         BFS root canonical — the BFS records the root identity as a strict self-root, \
         so the shifted FileWholeHash rejects the entry. Same-canonical invalidation is \
         lazy: with no eager upsert-time drain, the carrier rejects the edit on its own.",
    );
}

/// `RefCycleResultDb` rejects a warm entry after a content edit to a
/// VISITED (non-root) canonical the BFS walked. This is a
/// **characterization** test: it confirms a visited-canonical edit
/// rejects the warm entry — same-canonical invalidation is lazy, so the
/// entry physically survives the upsert and the carrier must reject it
/// on its own. It is not a strict-vs-lax discriminator — a visited
/// canonical is also carried in the legacy `DepSignature` rail (the
/// per-`Instantiate` dispatch dep-signature), which
/// `validate_dep_signature` already rejects on a content edit to a
/// tracked file. The strict-self-root discriminator for
/// `RefCycleResultDb` is the untracked-self-root test below.
#[test]
fn ref_cycle_db_visited_canonical_edit_rejects_warm_entry() {
    use crate::meta_resolve::ref_root_reaches_transitive_cycle_node;

    let host = VerterHost::new_standalone(HostConfig::default());
    let root = "/struct_carrier_qdb/rc_visit_root.ts";
    let helper = "/struct_carrier_qdb/rc_visit_helper.ts";
    upsert(
        &host,
        helper,
        "export type Helper<T> = { wrapped: T; next: Helper<T> };\n",
    );
    upsert(
        &host,
        root,
        "import type { Helper } from './rc_visit_helper';\nexport type Probe = Helper<number>;\n",
    );
    assert!(
        host.ensure_indexed_ready(helper).is_some(),
        "helper IndexedReady materialises",
    );
    assert!(
        host.ensure_indexed_ready(root).is_some(),
        "root IndexedReady materialises",
    );

    let ctx: &dyn ResolverContext = &host;
    let id = ref_cycle_decl_identity(&host, root, "Probe");

    // Cold BFS — walks `root` then `helper`; publishes a warm entry
    // whose self-root set includes BOTH the root and the visited
    // helper.
    let mut fence = Vec::new();
    let _ = ref_root_reaches_transitive_cycle_node(&id, ctx, &mut fence);
    let db = host.project_type_store().ref_cycle_db();
    let primed = crate::component_meta_caches::ref_cycle_db_peek(db, &id, ctx);
    assert!(
        primed.is_some(),
        "fixture invariant: the cold BFS admitted a warm RefCycleResultDb entry",
    );

    // Edit ONLY the visited helper file through the production
    // `upsert`. The BFS root file is untouched, so a producer that
    // rooted only the root would keep the entry valid.
    upsert(
        &host,
        helper,
        "export type Helper<T> = { wrapped: T; sibling: string; next: Helper<T> };\n",
    );
    assert!(
        host.ensure_indexed_ready(helper).is_some(),
        "helper IndexedReady re-materialises after the edit",
    );

    let ctx2: &dyn ResolverContext = &host;
    assert!(
        crate::component_meta_caches::ref_cycle_db_peek(db, &id, ctx2).is_none(),
        "RefCycleResultDb::peek MUST reject the warm entry after a content edit to a \
         VISITED (non-root) canonical the BFS walked — the visited helper is both a \
         strict self-root (recorded by the BFS) AND a legacy-rail dependency, and a \
         content edit to it rejects the entry on its own self-root.",
    );
}

/// `RefCycleResultDb` validates the BFS root's self-root **strictly**.
///
/// Discriminating property: a synthetic `RefCycleEntry` is planted
/// whose `facts` rail holds a `FileWholeHash` self-root for an
/// UNTRACKED root canonical and `self_root_canonicals = [root]`, with
/// an EMPTY legacy rail. The lax `validate(ctx)` routes the untracked
/// `FileWholeHash` through `StoreView::validates`, whose untracked-accept
/// arm returns `true`, and an empty legacy rail validates vacuously —
/// so the pre-self-root tree serves the entry warm. The strict
/// `validate_with_self_roots(ctx, [root])` routes the `FileWholeHash`
/// through `validates_self_root_whole_hash`, which rejects an untracked
/// self-root. The peek misses iff the validator is strict; reverting
/// `RefCycleResultDb::peek` to `validate(ctx)` flips this test.
///
/// (The legacy rail is left empty deliberately: the legacy
/// `DepSignature` validator's `WholeHash` arm rejects an untracked
/// canonical, so a `legacy`-rail untracked entry would mask the
/// strict-vs-lax distinction. The untracked-accept permissiveness lives
/// ONLY on the `facts` rail's `FileWholeHash` via
/// `StoreView::validates`.)
#[test]
fn ref_cycle_db_untracked_self_root_rejects_warm_entry() {
    use crate::component_meta_caches::RefCycleEntry;

    let host = host_with_unrelated_file();
    let root = "/struct_carrier_qdb/rc_never_loaded.ts";
    assert_untracked(&host, root);
    let ctx: &dyn ResolverContext = &host;
    let db = host.project_type_store().ref_cycle_db();
    let id = crate::semantic_query::DeclIdentity {
        canonical_id: Arc::from(root),
        whole_hash: PLANTED_HASH,
        decl_name: Arc::from("Probe"),
    };

    // Plant a synthetic entry: the carrier's facts rail holds a
    // self-root `FileWholeHash` for the untracked root, the legacy rail
    // is empty, and `self_root_canonicals` lists the root. A lax
    // validator admits this; the strict one does not.
    let facts: Arc<[FactVersionRef]> = Arc::from(vec![FactVersionRef::FileWholeHash {
        canonical_id: root.to_string(),
        hash: PLANTED_HASH,
    }]);
    let planted = Arc::new(RefCycleEntry {
        result: true,
        read_set_signature: crate::fact_signature_helpers::ReadSetSignature::new(facts),
        dispatch_dep_signature: std::sync::Arc::from(Vec::new()),
        self_root_canonicals: planted_self_root_canonicals(root),
        admission_seq: crate::bounded_query_retention::next_retention_seq(),
        // Live project generation — this test exercises the carrier's
        // strict self-root rejection, not the generation gate, so the
        // stamp must match the live generation.
        validated_at_generation: ctx.project_type_store().current_project_generation(),
    });
    db.entries().insert(id.clone(), planted);

    assert!(
        crate::component_meta_caches::ref_cycle_db_peek(db, &id, ctx).is_none(),
        "RefCycleResultDb::peek MUST reject a warm entry whose self-root FileWholeHash \
         names an UNTRACKED root canonical — the lax `validate` accepts the untracked \
         self-root and serves the entry stale; only the strict `validate_with_self_roots` \
         rejects it.",
    );
}

/// The ref-cycle BFS `ComputeAdmission::ReturnOnly` path MUST propagate
/// the BFS's read fence into the caller's `local_fence`.
///
/// Discriminating property. `ref_root_reaches_transitive_cycle_node`'s
/// cold path runs the BFS inside `ref_cycle_db_get_or_compute`'s
/// `compute` closure. When that closure refuses cache admission
/// (tracer overflow, an unrootable / torn self-root, or a
/// `RouteGeneration` fence dependency) it returns the computed bool via
/// `ComputeAdmission::ReturnOnly`. The caller's `Some(read)` arm merges
/// `read.dep_signature` into the caller's `local_fence` — so an outer
/// computation that called `ref_root_reaches_transitive_cycle_node` can
/// be cached without the files the BFS read unless the `ReturnOnly`
/// `CacheRead` carries the BFS fence.
///
/// This test forces the `ReturnOnly` exit AFTER the real BFS has run
/// (real `root` + `helper` files read, real `compute_fence` populated)
/// and asserts `local_fence` carries a `WholeHash` fact for BOTH the
/// BFS root and the visited helper, pinned to their CURRENT observed
/// content hashes.
///
/// - Pre-fix (`return_only_value` builds an empty `dep_signature`):
///   `local_fence` is empty — the BFS reads are dropped, the stale
///   outer-cache hole.
/// - Post-fix (`return_only_value` carries the `legacy` fence built
///   from `compute_fence`): `local_fence` carries the root + helper
///   `WholeHash` facts — observably equivalent to the proven `None`-arm
///   fallback that runs `local_fence.extend(fence)`, without a second
///   uncached BFS.
///
/// Reverting the `component_meta_caches.rs` `ReturnOnly` fence
/// propagation flips this test RED.
#[test]
fn ref_cycle_db_return_only_path_propagates_bfs_fence_to_caller() {
    use crate::meta_resolve::ref_root_reaches_transitive_cycle_node;
    use crate::semantic_query::DepVersion;

    let host = VerterHost::new_standalone(HostConfig::default());
    let root = "/struct_carrier_qdb/rc_returnonly_root.ts";
    let helper = "/struct_carrier_qdb/rc_returnonly_helper.ts";
    upsert(
        &host,
        helper,
        "export type Helper<T> = { wrapped: T; next: Helper<T> };\n",
    );
    upsert(
        &host,
        root,
        "import type { Helper } from './rc_returnonly_helper';\n\
         export type Probe = Helper<number>;\n",
    );
    assert!(
        host.ensure_indexed_ready(helper).is_some(),
        "helper IndexedReady materialises",
    );
    assert!(
        host.ensure_indexed_ready(root).is_some(),
        "root IndexedReady materialises",
    );

    // The CURRENT observed whole hashes of the two files the BFS reads.
    // The `ReturnOnly` fence must pin these exact hashes.
    let root_hash = host
        .shallow_file_state(root)
        .map(|s| s.whole_hash)
        .expect("root shallow state present");
    let helper_hash = host
        .shallow_file_state(helper)
        .map(|s| s.whole_hash)
        .expect("helper shallow state present");

    let ctx: &dyn ResolverContext = &host;
    let id = ref_cycle_decl_identity(&host, root, "Probe");

    // Force the cold `compute` closure down a `ComputeAdmission::ReturnOnly`
    // exit. The BFS still runs in full — `root` then `helper` are read,
    // `compute_fence` is populated — only the admission decision is
    // forced, reproducing the production overflow / unrootable-self-root
    // / RouteGeneration refusal contract.
    let _refusal = crate::component_meta_caches::force_ref_cycle_return_only_for_tests();
    let refusals_before = host
        .provenance
        .ref_cycle_overflow_refusals
        .load(std::sync::atomic::Ordering::Relaxed);

    let mut local_fence: Vec<(Arc<str>, DepVersion)> = Vec::new();
    let _ = ref_root_reaches_transitive_cycle_node(&id, ctx, &mut local_fence);

    let refusals_after = host
        .provenance
        .ref_cycle_overflow_refusals
        .load(std::sync::atomic::Ordering::Relaxed);
    assert!(
        refusals_after > refusals_before,
        "fixture invariant: the forced `ComputeAdmission::ReturnOnly` exit must have \
         fired (ref_cycle_overflow_refusals advanced) — without it the test would not \
         be exercising the ReturnOnly path at all",
    );
    let db = host.project_type_store().ref_cycle_db();
    assert!(
        crate::component_meta_caches::ref_cycle_db_peek(db, &id, ctx).is_none(),
        "fixture invariant: a `ReturnOnly` compute does NOT admit a warm entry — the \
         RefCycleResultDb slot stays empty",
    );

    // Discriminating assertion: the caller's `local_fence` carries a
    // `WholeHash` fact for the BFS root AND the visited helper, each
    // pinned to that file's CURRENT observed content hash.
    let fence_has = |canonical: &str, hash: [u8; 16]| {
        local_fence.iter().any(|(c, v)| {
            c.as_ref() == canonical && matches!(v, DepVersion::WholeHash(h) if *h == hash)
        })
    };
    assert!(
        fence_has(root, root_hash),
        "the `ReturnOnly` ref-cycle path MUST propagate the BFS's read fence into the \
         caller's `local_fence`: it must carry a `WholeHash` fact for the BFS root \
         `{root}` pinned to its current observed content hash. Pre-fix the \
         `ReturnOnly` `CacheRead` carried an empty `dep_signature`, so `local_fence` \
         was empty and an outer computation could be cached without the BFS's root \
         file — a stale-cache hole. local_fence = {local_fence:?}",
    );
    assert!(
        fence_has(helper, helper_hash),
        "the `ReturnOnly` ref-cycle path MUST propagate the BFS's read fence into the \
         caller's `local_fence`: the BFS walked through `root` into the visited helper \
         `{helper}`, so `local_fence` must also carry a `WholeHash` fact for the \
         helper pinned to its current observed content hash. local_fence = {local_fence:?}",
    );
}

// ===========================================================================
// `RouteOwnedShallowDb` — self-version-root (tier-1 content-hash) gate.
//
// `RouteOwnedShallowDb` is canonical-keyed with the self `whole_hash`
// carried INSIDE the entry. Its warm-read freshness gate
// (`route_owned_entry_is_fresh`) is tiered: tier 3 = `project_generation`,
// tier 1 = scheduler-authoritative content hash, tier 2 = workspace
// generation + `file_exists`. Tier 1 is the self-version root: it asserts
// `authoritative_current_content_hash(canonical) == entry.whole_hash`.
//
// The existing tiered-gate coverage in `cache_identity_invariants_tests`
// deliberately exercises a never-upserted canonical so `get_whole_hash`
// returns `None` and tier 2 decides — tier 1, the self-root content-hash
// comparison, is left uncovered there. The two tests below close that
// gap.
// ===========================================================================

/// Build a `RouteOwnedShallowEntry` whose generations match `host`'s
/// live state (so the tier-3 / tier-2 gates pass) but whose
/// `whole_hash` is whatever the caller plants — letting the tier-1
/// self-root comparison be the deciding tier.
fn route_owned_entry_with_whole_hash(
    host: &VerterHost,
    whole_hash: [u8; 16],
) -> crate::project_type_store::RouteOwnedShallowEntry {
    use rustc_hash::{FxHashMap, FxHashSet};
    let analysis = Arc::new(
        verter_compiler::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSource::default(),
    );
    crate::project_type_store::RouteOwnedShallowEntry {
        whole_hash,
        workspace_generation: host.ws().content_generation(),
        project_generation: host.project_type_store().current_project_generation(),
        raw_source: Arc::from(""),
        eval_source: Arc::from(""),
        cached_parse: None,
        snapshot: Arc::new(crate::types::FileAnalysisSnapshot::default()),
        external_type_analysis: Arc::clone(&analysis),
        shallow_state: Arc::new(crate::resolver_core::shallow_file_state::ShallowFileState {
            whole_hash,
            exports: FxHashMap::default(),
            wildcard_reexports: Vec::new(),
            symbols: FxHashMap::default(),
            value_symbols: FxHashMap::default(),
            import_locals: FxHashSet::default(),
            import_targets: FxHashMap::default(),
            analysis,
        }),
    }
}

/// `RouteOwnedShallowDb` — a stale self `whole_hash` is rejected by the
/// tier-1 self-version-root gate.
///
/// A real file is upserted, so the scheduler knows it and
/// `get_whole_hash` returns the authoritative current hash. A
/// `RouteOwnedShallowEntry` is then built with live generations (tier 3
/// and tier 2 both pass) but a *planted, stale* `whole_hash`. The
/// freshness gate must reject it on tier 1:
/// `authoritative_current_content_hash != entry.whole_hash`.
///
/// Discrimination property: tier 1 is the only tier that inspects the
/// entry's self `whole_hash`. If `RouteOwnedShallowEntry` did not carry
/// a self `whole_hash` — or the gate skipped the tier-1 comparison —
/// the entry would fall through to tier 2, whose `workspace_generation`
/// and `file_exists` clauses BOTH hold here, so the stale entry would
/// be wrongly accepted as fresh. The companion assertion plants the
/// genuine current hash and confirms tier 1 then accepts — proving the
/// gate is a real content-hash comparison, not an unconditional reject.
#[test]
fn route_owned_shallow_db_stale_whole_hash_rejects_warm_entry() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let canonical = "/self_root/route_owned_probe.ts";
    upsert(&host, canonical, "export const a = 1;\n");

    let authoritative = host
        .get_whole_hash(canonical)
        .expect("an upserted canonical must have a scheduler-authoritative hash");

    // A planted hash distinct from the authoritative one — a stale
    // self-root.
    let stale_hash = PLANTED_HASH;
    assert_ne!(
        stale_hash, authoritative,
        "fixture invariant: the planted stale hash must differ from the \
         authoritative current hash",
    );
    let stale_entry = route_owned_entry_with_whole_hash(&host, stale_hash);
    assert!(
        !host.route_owned_entry_is_fresh_for_test(canonical, &stale_entry),
        "the route-owned freshness gate MUST reject an entry whose self \
         `whole_hash` ({stale_hash:?}) mismatches the authoritative current \
         content hash ({authoritative:?}) — tier 1 is the self-version root. \
         An entry without a self `whole_hash`, or a gate skipping the tier-1 \
         comparison, would fall through to tier 2 (workspace_generation + \
         file_exists both hold here) and wrongly serve the stale entry",
    );

    // Companion: an entry carrying the genuine current hash passes
    // tier 1 — the gate is a content-hash comparison, not an
    // unconditional reject.
    let fresh_entry = route_owned_entry_with_whole_hash(&host, authoritative);
    assert!(
        host.route_owned_entry_is_fresh_for_test(canonical, &fresh_entry),
        "the route-owned freshness gate MUST accept an entry whose self \
         `whole_hash` equals the authoritative current content hash \
         ({authoritative:?}) — tier 1 self-root validation is a genuine \
         comparison, so a matching self-root passes",
    );
}

/// `RouteOwnedShallowDb` — a same-canonical content edit shifts the
/// authoritative hash, so a previously-fresh self `whole_hash` becomes
/// stale and the tier-1 gate rejects it.
///
/// This drives the self-version root end-to-end: the entry is built
/// fresh against the canonical's content, then the canonical is
/// re-upserted with edited content through the production `upsert`
/// (which performs no eager own-canonical eviction, so the entry
/// physically survives). The same entry — unchanged — must now be
/// rejected, because its self `whole_hash` no longer matches the
/// authoritative hash of the edited canonical.
///
/// Discrimination property: only an entry that carries a self
/// `whole_hash` validated against the *current* authoritative content
/// hash flips from fresh to stale across the edit. A gate that ignored
/// the self `whole_hash` would keep reporting the entry fresh after the
/// same-canonical edit.
#[test]
fn route_owned_shallow_db_self_root_rejects_warm_entry_after_same_canonical_edit() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let canonical = "/self_root/route_owned_edit_probe.ts";
    upsert(&host, canonical, "export const a = 1;\n");

    let hash_v1 = host
        .get_whole_hash(canonical)
        .expect("v1 canonical must have a scheduler-authoritative hash");
    let entry = route_owned_entry_with_whole_hash(&host, hash_v1);
    assert!(
        host.route_owned_entry_is_fresh_for_test(canonical, &entry),
        "fixture invariant: an entry carrying the v1 authoritative hash \
         must be fresh before the same-canonical edit",
    );

    // Same-canonical content edit through the production `upsert` — no
    // eager own-canonical eviction runs, so the route-owned freshness
    // gate's tier-1 self-root check is the only mechanism that can
    // reject the now-stale entry.
    upsert(&host, canonical, "export const a = 2;\n");

    let hash_v2 = host
        .get_whole_hash(canonical)
        .expect("v2 canonical must have a scheduler-authoritative hash");
    assert_ne!(
        hash_v1, hash_v2,
        "fixture invariant: the same-canonical content edit must shift the \
         authoritative content hash",
    );
    assert!(
        !host.route_owned_entry_is_fresh_for_test(canonical, &entry),
        "after a same-canonical content edit the route-owned entry's self \
         `whole_hash` ({hash_v1:?}) no longer matches the authoritative \
         current hash ({hash_v2:?}) — the tier-1 self-version root MUST \
         reject it. A gate that ignored the entry's self `whole_hash` would \
         keep reporting the stale entry fresh",
    );
}

// ===========================================================================
// Closure guard — every in-scope query-identity cache has a
// self-version-root discriminator.
// ===========================================================================

/// The complete set of in-scope query-identity caches that publish
/// canonical-keyed entries, paired with a coverage marker — the name of
/// a self-version-root discriminator test that proves the cache's
/// published entry carries (and the warm-read gate validates) a self
/// `FileWholeHash` for the keyed canonical.
///
/// `cache` is the `PROJECT_TYPE_STORE_DB_INVENTORY` identifier;
/// `marker` is a test-function name that must exist in one of the
/// self-root discriminator source files scanned below.
const IN_SCOPE_QUERY_IDENTITY_SELF_ROOT_COVERAGE: &[(&str, &str)] = &[
    (
        "semantic_graph",
        "resolve_decl_same_canonical_edit_rejects_warm_entry",
    ),
    (
        "declaration_lookup_db",
        "declaration_lookup_db_untracked_self_root_rejects_warm_entry",
    ),
    (
        "imported_registry_db",
        "imported_registry_db_untracked_self_root_rejects_warm_entry",
    ),
    (
        "resolvability_db",
        "resolvability_db_untracked_self_root_rejects_warm_entry",
    ),
    (
        "owner_collection_db",
        "owner_collection_db_untracked_self_root_rejects_warm_entry",
    ),
    (
        "prepared_target_db",
        "prepared_target_db_untracked_self_root_rejects_warm_entry",
    ),
    // Universal `ShapeCacheDb` — replaces the previously-
    // split `materialize_memo_db` (TypeExpr subject) +
    // `member_shape_cache_db` (SemanticNode subject). Both subjects
    // share the same cache substrate; each retains its own
    // self-root discriminator test under the unified DB name.
    (
        "shape_cache_db",
        "materialize_memo_db_untracked_self_root_rejects_warm_entry",
    ),
    (
        "shape_cache_db",
        "member_shape_cache_db_untracked_self_root_rejects_warm_entry",
    ),
    (
        "materialize_structure_db",
        "materialize_structure_db_planted_untracked_self_root_rejects_warm_entry",
    ),
    (
        "ref_cycle_db",
        "ref_cycle_db_untracked_self_root_rejects_warm_entry",
    ),
    (
        "route_owned_shallow",
        "route_owned_shallow_db_stale_whole_hash_rejects_warm_entry",
    ),
];

/// Closure guard — no in-scope query-identity cache can publish a
/// query-identity entry without a self-version root.
///
/// This is the deliverable-3 lock: it asserts the
/// [`IN_SCOPE_QUERY_IDENTITY_SELF_ROOT_COVERAGE`] manifest is both
/// *complete* (every in-scope cache has a discriminator) and *honest*
/// (every named discriminator actually exists, and every listed cache
/// is a real `ProjectTypeStore` DB).
///
/// Discrimination — three ways this guard fails on a regression:
///
/// 1. A cache name in the manifest is not in
///    `PROJECT_TYPE_STORE_DB_INVENTORY` — a typo or a renamed DB.
/// 2. A coverage marker names a test that no longer exists in the
///    self-root discriminator source files — deleting a cache's
///    self-version-root test trips the guard.
/// 3. A new query-identity-shaped DB is added to the inventory but not
///    to the manifest — the `expected_in_scope` cross-check fails,
///    forcing the author to either add a discriminator or justify the
///    DB's exclusion here.
#[test]
fn in_scope_query_identity_caches_all_have_self_root_coverage() {
    use std::fs;

    let inventory = crate::project_type_store::PROJECT_TYPE_STORE_DB_INVENTORY;

    // (1) Every manifest cache name is a real inventory DB.
    for (cache, _marker) in IN_SCOPE_QUERY_IDENTITY_SELF_ROOT_COVERAGE {
        assert!(
            inventory.contains(cache),
            "self-root coverage manifest names `{cache}`, which is not in \
             PROJECT_TYPE_STORE_DB_INVENTORY ({inventory:?}) — the cache was \
             renamed or the manifest entry is a typo",
        );
    }

    // (2) Every coverage marker names a test that exists in the
    //     self-root discriminator source files. The scan over source
    //     text makes the guard discriminating: deleting a cache's
    //     self-version-root test removes the marker and fails here.
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let self_root_sources = [
        "src/query_db_self_root_tests.rs",
        "src/semantic_graph_self_root_tests.rs",
        "src/query_identity_self_root_substrate_tests.rs",
    ];
    let mut corpus = String::new();
    for rel in self_root_sources {
        let path = std::path::Path::new(manifest_dir).join(rel);
        corpus.push_str(&fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!(
                "self-root source `{}` must be readable: {e}",
                path.display()
            )
        }));
        corpus.push('\n');
    }
    for (cache, marker) in IN_SCOPE_QUERY_IDENTITY_SELF_ROOT_COVERAGE {
        let needle = format!("fn {marker}");
        assert!(
            corpus.contains(&needle),
            "in-scope query-identity cache `{cache}` has no self-version-root \
             discriminator: the coverage marker test `{marker}` was not found \
             in any self-root discriminator source file ({self_root_sources:?}). \
             Every in-scope query-identity cache MUST publish entries carrying \
             a self `FileWholeHash` for the keyed canonical and have a test \
             proving the warm-read gate validates it",
        );
    }

    // (3) Cross-check: the in-scope set is exactly the query-identity
    //     subset of the inventory. `expected_in_scope` is the inventory
    //     minus the content-addressed artifact stores, the pure
    //     registries, and the tier-1 typed DBs that are not
    //     query-identity caches. A newly-registered query-identity DB
    //     that is not added to the manifest fails this assertion.
    let not_query_identity: &[&str] = &[
        // Content-addressed artifact stores (keyed by content hash) and
        // analysis-ready store — covered by content-addressed cache
        // tests, not the query-identity self-root family.
        "indexed",
        "analysis",
        // Owner import-surface + final component-meta result store —
        // version-rooted on the value via `fact_dep_signature`, covered
        // by the component-meta result-cache + canary suites.
        "owner_import_surfaces",
        "component_meta_results",
        "routes",
        "imported_roots",
        // Pure registry — no canonical-keyed query-identity entries.
        "intrinsic_registry",
        // App-config proof DB — keyed on app-config identity, not a
        // canonical query-identity cache.
        "app_config_no_override_proof",
        // Tier-1 typed DBs — content-domain / dep-closure-domain caches,
        // not query-identity caches.
        "type_resolution_context_db",
        "eval_env_cache_db",
        "compile_cache_db",
        "derived_raw_cache_db",
        "dependency_cache_db",
        "resolved_type_cache_db",
    ];
    let expected_in_scope: BTreeSet<&str> = inventory
        .iter()
        .copied()
        .filter(|name| !not_query_identity.contains(name))
        .collect();
    let manifest_caches: BTreeSet<&str> = IN_SCOPE_QUERY_IDENTITY_SELF_ROOT_COVERAGE
        .iter()
        .map(|(cache, _)| *cache)
        .collect();
    assert_eq!(
        manifest_caches, expected_in_scope,
        "the self-root coverage manifest must list EXACTLY the query-identity \
         subset of PROJECT_TYPE_STORE_DB_INVENTORY. A newly-registered \
         query-identity DB must be added to \
         IN_SCOPE_QUERY_IDENTITY_SELF_ROOT_COVERAGE with a discriminator \
         test; a DB that is genuinely not a query-identity cache must be \
         added to `not_query_identity` with a justification",
    );
}

// ===========================================================================
// Project-generation staleness — every cooperative-admission cache must
// reject an entry computed under a superseded project generation, and
// every reap-on-peek that the generation gate creates must route through
// the cache's COMPLETE removal cleanup (reverse-index unregister + ledger
// forget), not just the bare map removal.
//
// A `ProjectGeneration` reset (tsconfig / path-alias / SDK /
// workspace-folder change) bumps NO file content hash. The entries'
// file-content carriers (`ReadSetSignature` / `fact_dep_signature`)
// therefore validate a stale-by-project-generation entry vacuously
// forever. The `validated_at_generation` tag is the project-shape
// counterpart of the carrier check.
//
// The generation is advanced with the bare `bump_project_generation()`
// (NOT `bump_project_generation_and_evict()`) so the entry is NOT evicted
// by the bump itself — the warm read / peek must miss purely because the
// entry's stamped generation no longer equals the current one. A cache
// that carried no generation tag would leave the entry valid and serve it
// stale.
// ===========================================================================

/// `MaterializeStructureDb`'s stale-`peek` reap (generation mismatch)
/// routes through the cache's COMPLETE removal cleanup — every
/// `canonical_to_keys` reverse-index registration the reaped entry held
/// is unregistered and every now-empty shard is pruned.
///
/// Discriminating property — a planted entry references TWO canonicals,
/// so `register_post_publish` creates TWO `canonical_to_keys` shards. A
/// `bump_project_generation()` makes the entry generation-stale; the next
/// `peek` reaps it. After the reap:
///
/// - A reap that drops only the map entry + the retention ledger (the
///   pre-fix `MaterializeStructureDb::peek` body) leaves BOTH dead
///   reverse-index shards resident — `canonical_to_keys_shard_count_for_test()`
///   reads 2 and the assertion FAILS.
/// - A reap routed through `unregister_post_publish` (loops the entry's
///   `canonical_ids()`, prunes each shard) empties both shards' inner
///   maps and drops the outer shards — the count reads 0 and the
///   assertion PASSES.
#[test]
fn materialize_structure_peek_stale_reap_cleans_every_reverse_index_shard() {
    use crate::component_meta_caches::MaterializeStructureEntry;
    use crate::component_meta_materialize::{
        MaterializationScope, MaterializeOutcome, MaterializeStructureCacheKey,
    };
    use crate::fact_signature_helpers::ReadSetSignature;
    use crate::semantic_query::{ProjectionMode, SemanticNodeId};

    let host = VerterHost::new_standalone(HostConfig::default());
    let ctx: &dyn ResolverContext = &host;
    let db = host.project_type_store().materialize_structure_db();

    let key = MaterializeStructureCacheKey {
        scope_canonical_id: Arc::from("/owner.ts"),
        base: SemanticNodeId(0),
        scope_axis: MaterializationScope::TopLevel,
        mode: ProjectionMode::Shallow,
    };
    // A fact rail naming TWO distinct cross-file-dependency canonicals.
    // `register_post_publish` loops `canonical_ids()` (which folds in
    // both the legacy and the fact rail) and creates one reverse-index
    // shard per canonical, so the entry holds two registrations. The
    // canonicals are untracked dependencies (NOT self-roots), so the
    // carrier routes them through the lazy "untracked → accept" path
    // and the entry is a valid warm hit until the generation gate
    // rejects it.
    let facts: Arc<[FactVersionRef]> = Arc::from(vec![
        FactVersionRef::FileWholeHash {
            canonical_id: "/dep-a.ts".to_string(),
            hash: [1; 16],
        },
        FactVersionRef::FileWholeHash {
            canonical_id: "/dep-b.ts".to_string(),
            hash: [2; 16],
        },
    ]);
    let generation_at_compute = host.project_type_store().current_project_generation();
    let entry = Arc::new(MaterializeStructureEntry {
        outcome: MaterializeOutcome::Miss(SemanticNodeId(0)),
        read_set_signature: ReadSetSignature::new(facts),
        dispatch_dep_signature: Arc::from(Vec::new()),
        self_root_canonicals: Arc::from(Vec::<Arc<str>>::new()),
        admission_seq: crate::bounded_query_retention::next_retention_seq(),
        validated_at_generation: generation_at_compute,
    });
    db.entries().insert(key.clone(), Arc::clone(&entry));
    db.bump_live_counter();
    db.register_post_publish(key.clone(), &entry.read_set_signature, entry.admission_seq);
    assert_eq!(
        db.canonical_to_keys_shard_count_for_test(),
        2,
        "fixture invariant: a two-canonical entry registers two reverse-index shards",
    );
    // The entry's carrier validates against current store state (the
    // planted canonicals are untracked → lazy-accepted), so `peek` would
    // HIT were it not for the generation gate.
    assert!(
        db.peek(&key, ctx).is_some(),
        "fixture invariant: the planted entry is a warm hit before the generation bump",
    );

    // Advance the project generation WITHOUT evicting — the entry stays
    // resident; the next `peek` must reap it on the generation tag alone.
    let g_after = host.project_type_store().bump_project_generation();
    assert!(
        g_after > generation_at_compute,
        "fixture invariant: the project generation advanced past the stamped value",
    );

    // Generation-mismatch `peek` — reaps the entry.
    assert!(
        db.peek(&key, ctx).is_none(),
        "fixture invariant: the generation-stale entry must miss on peek",
    );
    assert!(
        db.entries().get(&key).is_none(),
        "fixture invariant: the stale entry is reaped from the entry map",
    );

    assert_eq!(
        db.canonical_to_keys_shard_count_for_test(),
        0,
        "INCOMPLETE STALE REAP: `MaterializeStructureDb::peek`'s \
         generation-mismatch reap dropped the map entry and the retention \
         ledger record but left the reaped entry's `canonical_to_keys` \
         reverse-index registrations resident. The entry referenced two \
         canonicals, so two dead shards accumulate and keep doing \
         unnecessary invalidation work — a later invalidation of one shard \
         cannot reach the others because the entry is already gone. The \
         stale-peek reap must route through the same \
         `unregister_post_publish` cleanup the cooperative-removal path \
         uses, which loops the entry's `canonical_ids()` and prunes every \
         emptied shard.",
    );
}

/// `RefCycleResultDb`'s stale-`peek` reap (generation mismatch) routes
/// through the cache's COMPLETE removal cleanup — the `RefCycleResultDb`
/// mirror of `materialize_structure_peek_stale_reap_cleans_every_reverse_index_shard`.
#[test]
fn ref_cycle_peek_stale_reap_cleans_every_reverse_index_shard() {
    use crate::component_meta_caches::RefCycleEntry;
    use crate::fact_signature_helpers::ReadSetSignature;
    use crate::semantic_query::{DeclIdentity, HashValue};

    let host = VerterHost::new_standalone(HostConfig::default());
    let ctx: &dyn ResolverContext = &host;
    let db = host.project_type_store().ref_cycle_db();

    let key = DeclIdentity {
        canonical_id: Arc::from("/owner.ts"),
        whole_hash: HashValue::default(),
        decl_name: Arc::from("RootHelper"),
    };
    // A fact rail naming TWO distinct cross-file-dependency canonicals —
    // two reverse-index shards registered. Untracked dependencies (NOT
    // self-roots) route through the lazy "untracked → accept" path, so
    // the entry is a valid warm hit until the generation gate rejects it.
    let facts: Arc<[FactVersionRef]> = Arc::from(vec![
        FactVersionRef::FileWholeHash {
            canonical_id: "/dep-a.ts".to_string(),
            hash: [1; 16],
        },
        FactVersionRef::FileWholeHash {
            canonical_id: "/dep-b.ts".to_string(),
            hash: [2; 16],
        },
    ]);
    let generation_at_compute = host.project_type_store().current_project_generation();
    let entry = Arc::new(RefCycleEntry {
        result: false,
        read_set_signature: ReadSetSignature::new(facts),
        dispatch_dep_signature: Arc::from(Vec::new()),
        self_root_canonicals: Arc::from(Vec::<Arc<str>>::new()),
        admission_seq: crate::bounded_query_retention::next_retention_seq(),
        validated_at_generation: generation_at_compute,
    });
    db.entries().insert(key.clone(), Arc::clone(&entry));
    db.bump_live_counter();
    db.register_post_publish(key.clone(), &entry.read_set_signature, entry.admission_seq);
    assert_eq!(
        db.canonical_to_keys_shard_count_for_test(),
        2,
        "fixture invariant: a two-canonical entry registers two reverse-index shards",
    );
    assert!(
        db.peek(&key, ctx).is_some(),
        "fixture invariant: the planted entry is a warm hit before the generation bump",
    );

    let g_after = host.project_type_store().bump_project_generation();
    assert!(
        g_after > generation_at_compute,
        "fixture invariant: the project generation advanced past the stamped value",
    );

    assert!(
        db.peek(&key, ctx).is_none(),
        "fixture invariant: the generation-stale entry must miss on peek",
    );
    assert!(
        db.entries().get(&key).is_none(),
        "fixture invariant: the stale entry is reaped from the entry map",
    );

    assert_eq!(
        db.canonical_to_keys_shard_count_for_test(),
        0,
        "INCOMPLETE STALE REAP: `RefCycleResultDb::peek`'s \
         generation-mismatch reap dropped the map entry and the budget \
         record but left the reaped entry's `canonical_to_keys` \
         reverse-index registrations resident — dead shards accumulate. \
         The stale-peek reap must route through `unregister_post_publish`, \
         which loops the entry's `canonical_ids()` and prunes every \
         emptied shard.",
    );
}

/// `ImportedRegistryDb::peek` rejects an entry computed under a
/// superseded project generation.
///
/// Discriminating property — an entry is primed through the production
/// `get_or_compute_admit` cold path (which stamps `validated_at_generation`
/// with the project generation snapshotted before dispatch). A bare
/// `bump_project_generation()` advances the counter without evicting. The
/// next `peek`:
///
/// - With no generation tag (the pre-fix `ImportedRegistryEntry`) validates
///   the entry by its file-content-only `fact_dep_signature` alone — the
///   carrier still matches, so `peek` returns a stale HIT and the
///   assertion FAILS.
/// - With the `validated_at_generation` tag checked at `peek`, the stamped
///   generation no longer equals the current one — `peek` MISSES and the
///   assertion PASSES.
#[test]
fn imported_registry_peek_rejects_entry_from_superseded_generation() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let canonical = "/superseded_gen/imported_registry.ts";
    upsert(
        &host,
        canonical,
        "export type Probe = number;\nexport const probe = 1;\n",
    );
    assert!(
        host.ensure_indexed_ready(canonical).is_some(),
        "fixture invariant: canonical IndexedReady materialises",
    );

    let ctx: &dyn ResolverContext = &host;
    let db = host.project_type_store().imported_registry_db();
    let key: crate::component_meta_caches::ImportedRegistryKey =
        (Arc::from(canonical), Arc::from("Probe"));

    // Prime the cache through the production cold path — the `Cacheable`
    // arm stamps `validated_at_generation` with the project generation
    // snapshotted before the compute dispatches, exactly as the
    // production producer (`registry_decl.rs`) does, and registers the
    // reverse index.
    let g_before = host.project_type_store().current_project_generation();
    let primed = db.get_or_compute_admit(&key, ctx, || {
        let validated_at_generation = host.project_type_store().current_project_generation();
        crate::cooperative_admission::ComputeAdmission::Cacheable(
            crate::component_meta_caches::ImportedRegistryEntry {
                value: None,
                fact_dep_signature: empty_fact_signature(),
                validated_at_generation,
            },
        )
    });
    assert!(
        primed.is_some(),
        "fixture invariant: the cold cooperative admission publishes the entry",
    );
    assert_eq!(
        db.live_count(),
        1,
        "fixture invariant: the primed entry is admitted to ImportedRegistryDb",
    );
    assert!(
        db.peek(&key, ctx).is_some(),
        "fixture invariant: the primed entry is a warm peek hit before the generation bump",
    );

    // Advance the project generation WITHOUT evicting any cache.
    let g_after = host.project_type_store().bump_project_generation();
    assert!(
        g_after > g_before,
        "fixture invariant: the project generation advanced past the stamped value",
    );
    assert_eq!(
        db.live_count(),
        1,
        "fixture invariant: bump_project_generation does NOT evict the entry — the \
         peek must reject it on the generation tag alone",
    );

    assert!(
        db.peek(&key, ctx).is_none(),
        "PROJECT-GENERATION STALENESS: `ImportedRegistryDb::peek` served an \
         entry computed under a superseded project generation. A \
         `ProjectGeneration` reset (tsconfig / path-alias / SDK / \
         workspace-folder change) bumps no file content, so the entry's \
         file-content-only `fact_dep_signature` validates the stale entry \
         vacuously forever. `peek` must additionally reject the entry when \
         its `validated_at_generation` no longer equals the live project \
         generation.",
    );
}

/// `ImportedRegistryDb`'s cooperative warm-hit reject driven by a
/// project-generation mismatch routes through the cache's COMPLETE
/// removal cleanup — the substrate's `removal_cleanup` closure
/// unregisters the reaped entry's `CanonicalIndex` reverse-index
/// registration.
///
/// `ImportedRegistryDb` is the only fence-less cache that carries a
/// reverse index, so it is the one Part-C coupling concern: adding the
/// generation gate to the cooperative `validate` closure creates a new
/// reject path, and that reject must clean the reverse index, not just
/// drop the map entry. The cooperative substrate already routes a
/// `validate`-rejected entry through `remove_published_entry_with_cleanup`,
/// which runs the cache's `removal_cleanup` (here:
/// `canonical_index.unregister`) — so the generation gate riding inside
/// `validate` reuses that complete cleanup.
///
/// Discriminating property — an entry is primed through the production
/// cold path (stamps `validated_at_generation`, registers the reverse
/// index). A bare `bump_project_generation()` advances the counter. The
/// next `get_or_compute_admit` reaches its warm-hit `validate` arm; its
/// `compute` closure returns `ReturnOnly` (no republish, so the only
/// reverse-index mutation observable afterwards is the reap's
/// `unregister`):
///
/// - With no generation gate in `validate` (the pre-fix closure) the
///   warm-hit `validate` accepts the stale entry by its file-content-only
///   `fact_dep_signature`; the entry is served, never reaped, and its
///   reverse-index registration survives — `reverse_index_contains_for_test`
///   reads `true` and the assertion FAILS.
/// - With the generation gate in `validate`, the stale entry is rejected,
///   the substrate's `remove_published_entry_with_cleanup` removes it AND
///   runs `removal_cleanup` → `canonical_index.unregister`; the
///   registration is gone — `reverse_index_contains_for_test` reads
///   `false` and the assertion PASSES.
#[test]
fn imported_registry_cooperative_generation_reject_cleans_reverse_index() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let canonical = "/superseded_gen/imported_registry_reverse_index.ts";
    upsert(
        &host,
        canonical,
        "export type Probe = number;\nexport const probe = 1;\n",
    );
    assert!(
        host.ensure_indexed_ready(canonical).is_some(),
        "fixture invariant: canonical IndexedReady materialises",
    );

    let ctx: &dyn ResolverContext = &host;
    let db = host.project_type_store().imported_registry_db();
    let key: crate::component_meta_caches::ImportedRegistryKey =
        (Arc::from(canonical), Arc::from("Probe"));

    // Prime the cache through the production cold path — stamps
    // `validated_at_generation` and registers `key` in the per-canonical
    // reverse index.
    let primed = db.get_or_compute_admit(&key, ctx, || {
        let validated_at_generation = host.project_type_store().current_project_generation();
        crate::cooperative_admission::ComputeAdmission::Cacheable(
            crate::component_meta_caches::ImportedRegistryEntry {
                value: None,
                fact_dep_signature: empty_fact_signature(),
                validated_at_generation,
            },
        )
    });
    assert!(
        primed.is_some(),
        "fixture invariant: the cold cooperative admission publishes the entry",
    );
    assert!(
        db.reverse_index_contains_for_test(&key),
        "fixture invariant: the primed entry registered `key` in the \
         per-canonical reverse index",
    );

    // Advance the project generation WITHOUT evicting any cache.
    let g_after_bump = host.project_type_store().bump_project_generation();
    assert!(
        g_after_bump > 0,
        "fixture invariant: the project generation advanced",
    );

    // A second cooperative admission: its warm-hit `validate` arm
    // re-reads the stale entry. The `compute` closure returns
    // `ReturnOnly` so nothing republishes — the only reverse-index
    // mutation observable afterwards is the reap's `unregister`.
    let mut cold_ran = false;
    let _ = db.get_or_compute_admit(&key, ctx, || {
        cold_ran = true;
        crate::cooperative_admission::ComputeAdmission::ReturnOnly(None)
    });
    assert!(
        cold_ran,
        "the warm-hit `validate` arm MUST reject the generation-stale entry so the \
         cooperative cold path runs — a `validate` that accepted the stale entry on \
         its file-content-only `fact_dep_signature` would short-circuit before \
         `compute`.",
    );

    assert!(
        !db.reverse_index_contains_for_test(&key),
        "INCOMPLETE COOPERATIVE REAP: `ImportedRegistryDb`'s cooperative \
         warm-hit reject of a generation-stale entry dropped the map entry \
         but left its `CanonicalIndex` reverse-index registration resident. \
         The generation gate rides inside the cooperative `validate` \
         closure, and a `validate` rejection routes through the substrate's \
         `remove_published_entry_with_cleanup`, which runs `removal_cleanup` \
         → `canonical_index.unregister`. The reverse-index registration \
         must be gone after the reap.",
    );
    assert_eq!(
        db.live_count(),
        0,
        "the generation-stale entry must be reaped from the entry map and \
         the `ReturnOnly` cold outcome must not republish",
    );
}

// ---------------------------------------------------------------------------
// Cooperative-admission `live_counter` accounting invariant.
//
// The shared `component_meta_cache_live` counter must equal the number of
// entries actually live in the cache maps on EVERY admission path. The
// failure mode under test: a cold `cooperative_get_or_insert` compute that
// fails `revalidate_after_compute` (a project-generation reset landed
// during the cold window) publishes NO map entry — the substrate marks the
// slot failed and returns `None` WITHOUT running `removal_cleanup` (nothing
// was inserted). If the live-counter `fetch_add` happens inside the
// `compute` closure (before publication), that failed cold compute leaks a
// permanent `+1` against the shared counter with no backing map entry.
//
// The fix moves the `fetch_add` into the substrate's winner-only
// `post_publish` callback — fired exactly once, AFTER `map.insert` and a
// successful `revalidate_after_compute`. `post_publish` is structurally
// unreachable on the revalidation-fail path, so the leak is impossible by
// construction: an entry contributes `+1` exactly while it is live in the
// map, paired with the `removal_cleanup` decrement.
//
// Each test below drives one of the `cooperative_get_or_insert` engine
// DBs through its production `get_or_compute`. The `compute` closure bumps
// the project generation INSIDE itself: `get_or_compute` snapshots the
// generation as `G` before the cooperative call, the closure advances it
// to `G+1` and stamps the entry `validated_at_generation: G`, and the
// substrate's `revalidate_after_compute` then rejects `G == G+1`. The
// publish is skipped deterministically — no map entry, no `post_publish`.
// The discriminating assertion: the shared `component_meta_cache_live`
// counter is UNCHANGED across the failed cold compute. Pre-fix the
// in-`compute` `fetch_add` leaks `+1` (RED); post-fix the bump rides
// `post_publish`, which never fires, so the counter holds (GREEN).

/// Read the shared `component_meta_cache_live` counter — the value that
/// must always equal the number of entries live across every
/// component-meta cache map.
fn component_meta_cache_live(host: &VerterHost) -> u64 {
    host.project_type_store()
        .counters
        .component_meta_cache_live
        .load(std::sync::atomic::Ordering::Relaxed)
}

/// `DeclarationLookupDb` — a cold `cooperative_get_or_insert` compute
/// whose `revalidate_after_compute` fails (project-generation reset
/// mid-compute) must NOT leak the shared `component_meta_cache_live`
/// counter: the entry is never published, so it must contribute `0`.
#[test]
fn declaration_lookup_failed_revalidation_does_not_leak_live_counter() {
    let host = host_with_unrelated_file();
    let c = "/live_counter_qdb/declaration_lookup.ts";
    let ctx: &dyn ResolverContext = &host;
    let db = host.project_type_store().declaration_db();
    let key = (Arc::<str>::from(c), Arc::<str>::from("Probe"));

    let counter_before = component_meta_cache_live(&host);
    let map_before = db.live_count();

    // Cold compute: advance the project generation INSIDE the closure.
    // `get_or_compute` snapshotted the generation BEFORE this closure ran,
    // so the entry is stamped with the now-superseded generation and the
    // substrate's `revalidate_after_compute` rejects the publish.
    let outcome = db.get_or_compute(&key, ctx, || {
        host.project_type_store().bump_project_generation();
        Some((decl("stale", c), empty_fact_signature()))
    });
    assert!(
        outcome.is_none(),
        "fixture invariant: the cold compute's `revalidate_after_compute` must \
         reject the entry (it was stamped with a superseded project generation), \
         so `get_or_compute` returns `None` and nothing is published",
    );
    assert_eq!(
        db.live_count(),
        map_before,
        "fixture invariant: a rejected cold compute publishes NO map entry",
    );
    assert_eq!(
        component_meta_cache_live(&host),
        counter_before,
        "LIVE-COUNTER LEAK: `DeclarationLookupDb`'s cold `cooperative_get_or_insert` \
         compute incremented the shared `component_meta_cache_live` counter, but its \
         `revalidate_after_compute` then rejected the entry — no map entry was \
         published and the substrate ran no `removal_cleanup`. The counter is now \
         permanently overcounted with no backing entry. The live-counter bump must \
         ride the winner-only `post_publish` callback (fired only after a successful \
         publish), not the `compute` closure.",
    );
}

/// `ResolvabilityDb` — failed-revalidation cold compute must not leak the
/// shared live counter. See
/// [`declaration_lookup_failed_revalidation_does_not_leak_live_counter`].
#[test]
fn resolvability_failed_revalidation_does_not_leak_live_counter() {
    let host = host_with_unrelated_file();
    let c = "/live_counter_qdb/resolvability.ts";
    let ctx: &dyn ResolverContext = &host;
    let db = host.project_type_store().resolvable_db();
    let key = (Arc::<str>::from(c), Arc::<str>::from("Probe"));

    let counter_before = component_meta_cache_live(&host);
    let map_before = db.live_count();

    let outcome = db.get_or_compute(&key, ctx, || {
        host.project_type_store().bump_project_generation();
        Some((true, empty_fact_signature()))
    });
    assert!(
        outcome.is_none(),
        "fixture invariant: the cold compute's `revalidate_after_compute` must \
         reject the entry, so `get_or_compute` returns `None`",
    );
    assert_eq!(
        db.live_count(),
        map_before,
        "fixture invariant: a rejected cold compute publishes NO map entry",
    );
    assert_eq!(
        component_meta_cache_live(&host),
        counter_before,
        "LIVE-COUNTER LEAK: `ResolvabilityDb`'s cold `cooperative_get_or_insert` \
         compute leaked the shared `component_meta_cache_live` counter on a \
         `revalidate_after_compute` rejection. The bump must ride `post_publish`.",
    );
}

/// `OwnerCollectionDb` — failed-revalidation cold compute must not leak the
/// shared live counter. See
/// [`declaration_lookup_failed_revalidation_does_not_leak_live_counter`].
#[test]
fn owner_collection_failed_revalidation_does_not_leak_live_counter() {
    let host = host_with_unrelated_file();
    let c = "/live_counter_qdb/owner_collection.ts";
    let ctx: &dyn ResolverContext = &host;
    let db = host.project_type_store().owner_collection_db();
    let key = (Arc::<str>::from(c), Arc::<str>::from("Probe"));

    let counter_before = component_meta_cache_live(&host);
    let map_before = db.live_count();

    let outcome = db.get_or_compute(&key, ctx, || {
        host.project_type_store().bump_project_generation();
        Some((None, empty_fact_signature()))
    });
    assert!(
        outcome.is_none(),
        "fixture invariant: the cold compute's `revalidate_after_compute` must \
         reject the entry, so `get_or_compute` returns `None`",
    );
    assert_eq!(
        db.live_count(),
        map_before,
        "fixture invariant: a rejected cold compute publishes NO map entry",
    );
    assert_eq!(
        component_meta_cache_live(&host),
        counter_before,
        "LIVE-COUNTER LEAK: `OwnerCollectionDb`'s cold `cooperative_get_or_insert` \
         compute leaked the shared `component_meta_cache_live` counter on a \
         `revalidate_after_compute` rejection. The bump must ride `post_publish`.",
    );
}

/// `PreparedTargetDb` — failed-revalidation cold compute must not leak the
/// shared live counter. See
/// [`declaration_lookup_failed_revalidation_does_not_leak_live_counter`].
#[test]
fn prepared_target_failed_revalidation_does_not_leak_live_counter() {
    let host = host_with_unrelated_file();
    let c = "/live_counter_qdb/prepared_target.ts";
    let ctx: &dyn ResolverContext = &host;
    let db = host.project_type_store().prepared_target_db();
    let key = PreparedTargetCacheKey {
        active_scope_canonical_id: Arc::from(c),
        decl_canonical_id: Arc::from(c),
        decl_symbol_name: Arc::from("Probe"),
        requested_name: Arc::from("Probe"),
    };

    let counter_before = component_meta_cache_live(&host);
    let map_before = db.live_count();

    let outcome = db.get_or_compute(&key, ctx, || {
        host.project_type_store().bump_project_generation();
        Some((None, empty_fact_signature(), empty_self_root_canonicals()))
    });
    assert!(
        outcome.is_none(),
        "fixture invariant: the cold compute's `revalidate_after_compute` must \
         reject the entry, so `get_or_compute` returns `None`",
    );
    assert_eq!(
        db.live_count(),
        map_before,
        "fixture invariant: a rejected cold compute publishes NO map entry",
    );
    assert_eq!(
        component_meta_cache_live(&host),
        counter_before,
        "LIVE-COUNTER LEAK: `PreparedTargetDb`'s cold `cooperative_get_or_insert` \
         compute leaked the shared `component_meta_cache_live` counter on a \
         `revalidate_after_compute` rejection. The bump must ride `post_publish`.",
    );
}

/// `MaterializeMemoDb` — failed-revalidation cold compute must not leak the
/// shared live counter. See
/// [`declaration_lookup_failed_revalidation_does_not_leak_live_counter`].
#[test]
fn materialize_memo_failed_revalidation_does_not_leak_live_counter() {
    let host = host_with_unrelated_file();
    let c = "/live_counter_qdb/materialize_memo.ts";
    let ctx: &dyn ResolverContext = &host;
    let db = host.project_type_store().shape_cache_db();
    let key = crate::component_meta_caches::ShapeCacheKey::type_expr_whole(
        Arc::<str>::from(c),
        Arc::new(TypeExpr::Unknown { raw: String::new() }),
        ProjectionMode::Shallow,
    );

    let counter_before = component_meta_cache_live(&host);
    let map_before = db.live_count();

    let outcome = db.get_or_compute(&key, ctx, || {
        host.project_type_store().bump_project_generation();
        Some((
            MaterializedTypeExpr {
                node_id: None,
                type_expr: TypeExpr::Unknown { raw: String::new() },
                dep_signature: Arc::from([] as [(Arc<str>, crate::semantic_query::DepVersion); 0]),
                cache_suppress: false,
            },
            empty_fact_signature(),
        ))
    });
    assert!(
        outcome.is_none(),
        "fixture invariant: the cold compute's `revalidate_after_compute` must \
         reject the entry, so `get_or_compute` returns `None`",
    );
    assert_eq!(
        db.live_count(),
        map_before,
        "fixture invariant: a rejected cold compute publishes NO map entry",
    );
    assert_eq!(
        component_meta_cache_live(&host),
        counter_before,
        "LIVE-COUNTER LEAK: `ShapeCacheDb`'s cold `cooperative_get_or_insert` \
         compute leaked the shared `component_meta_cache_live` counter on a \
         `revalidate_after_compute` rejection. The bump must ride `post_publish`.",
    );
}

/// Class-level invariant: after the `cooperative_get_or_insert` engine
/// DBs each absorb a failed-revalidation cold compute, the shared
/// `component_meta_cache_live` counter still equals the TOTAL number of
/// entries live across every component-meta cache map. This is the
/// whole-class consistency check — `live_counter == sum of live map
/// entries` on the failed-revalidation path, for all 8 DBs at once.
///
/// Pre-fix every one of the 8 failed computes leaks `+1`, so the counter
/// ends `8` above the true live total (RED). Post-fix every bump rides
/// `post_publish` (never fired on the rejection path), so the counter
/// equals the true total (GREEN).
#[test]
fn cooperative_get_or_insert_dbs_keep_live_counter_equal_to_map_total() {
    let host = host_with_unrelated_file();
    let ctx: &dyn ResolverContext = &host;
    let store = host.project_type_store();

    // Sum of every component-meta cache map that contributes to the
    // shared `component_meta_cache_live` counter.
    let live_map_total = |store: &crate::project_type_store::ProjectTypeStore| -> usize {
        store.imported_registry_db().live_count()
            + store.declaration_db().live_count()
            + store.resolvable_db().live_count()
            + store.owner_collection_db().live_count()
            + store.prepared_target_db().live_count()
            + store.shape_cache_db().live_count()
            + store.materialize_structure_db().live_count()
            + store.ref_cycle_db().entries().len()
    };

    // Drive a failed-revalidation cold compute through each of the
    // `cooperative_get_or_insert` engine DBs.
    let bump = || {
        host.project_type_store().bump_project_generation();
    };

    let _ = store.declaration_db().get_or_compute(
        &(Arc::<str>::from("/lc_total/decl.ts"), Arc::<str>::from("P")),
        ctx,
        || {
            bump();
            Some((decl("stale", "/lc_total/decl.ts"), empty_fact_signature()))
        },
    );
    let _ = store.resolvable_db().get_or_compute(
        &(
            Arc::<str>::from("/lc_total/resolv.ts"),
            Arc::<str>::from("P"),
        ),
        ctx,
        || {
            bump();
            Some((true, empty_fact_signature()))
        },
    );
    let _ = store.owner_collection_db().get_or_compute(
        &(
            Arc::<str>::from("/lc_total/owner.ts"),
            Arc::<str>::from("P"),
        ),
        ctx,
        || {
            bump();
            Some((None, empty_fact_signature()))
        },
    );
    let _ = store.prepared_target_db().get_or_compute(
        &PreparedTargetCacheKey {
            active_scope_canonical_id: Arc::from("/lc_total/ptarget.ts"),
            decl_canonical_id: Arc::from("/lc_total/ptarget.ts"),
            decl_symbol_name: Arc::from("P"),
            requested_name: Arc::from("P"),
        },
        ctx,
        || {
            bump();
            Some((None, empty_fact_signature(), empty_self_root_canonicals()))
        },
    );
    let _ = store.shape_cache_db().get_or_compute(
        &crate::component_meta_caches::ShapeCacheKey::type_expr_whole(
            Arc::<str>::from("/lc_total/memo.ts"),
            Arc::new(TypeExpr::Unknown { raw: String::new() }),
            ProjectionMode::Shallow,
        ),
        ctx,
        || {
            bump();
            Some((
                MaterializedTypeExpr {
                    node_id: None,
                    type_expr: TypeExpr::Unknown { raw: String::new() },
                    dep_signature: Arc::from(
                        [] as [(Arc<str>, crate::semantic_query::DepVersion); 0]
                    ),
                    cache_suppress: false,
                },
                empty_fact_signature(),
            ))
        },
    );

    assert_eq!(
        component_meta_cache_live(&host) as usize,
        live_map_total(store),
        "LIVE-COUNTER CLASS LEAK: after driving a failed-revalidation cold compute \
         through all 5 `cooperative_get_or_insert` engine DBs, the shared \
         `component_meta_cache_live` counter no longer equals the total number of \
         entries live across the component-meta cache maps. Each failed cold compute \
         that bumped the counter inside its `compute` closure (before the \
         substrate's `revalidate_after_compute` rejected the publish) leaked `+1`. \
         The live-counter bump must ride the winner-only `post_publish` callback so \
         the counter equals the live map total on every admission path.",
    );
}

/// `ComponentMetaResultDb::get_with_view` rejects an entry whose
/// `validated_at_generation` no longer equals the live project
/// generation — a `ProjectGeneration` reset bumps no file content, so
/// the carrier alone cannot detect it.
///
/// Mirror of `materialize_structure_peek_rejects_entry_from_superseded_generation`:
/// plant a `ComponentMetaResultEntry` with a valid carrier (empty
/// signature validates vacuously) tagged with the CURRENT project
/// generation. The first lookup HITs. Then `bump_project_generation()`
/// (the bare version that increments the counter WITHOUT clearing the
/// cache) — the planted entry's carrier is still valid, only its
/// generation stamp is now stale. The second lookup MUST MISS purely
/// because of the generation-stamp mismatch. Without the gate,
/// `get_with_view`'s carrier check alone still passes (no file
/// content changed) and the stale entry is served.
#[test]
fn component_meta_result_db_get_with_view_rejects_entry_from_superseded_generation() {
    use crate::component_meta_result_db::{
        CachedComponentMetaResult, ComponentMetaResultEntry, ComponentMetaResultKey,
        ResolutionTemplate,
    };

    let host = VerterHost::new_standalone(HostConfig::default());
    let owner = "/gen_peek_cmr/owner.vue";
    upsert(&host, owner, "<script setup lang=\"ts\"></script>\n");

    let store = host.project_type_store();
    let key = ComponentMetaResultKey {
        owner_canonical: Arc::from(owner),
        options_fingerprint: [0u8; 16],
    };
    let owner_whole_hash = [0xCDu8; 16];
    let gen0 = store.current_project_generation();
    let analysis: verter_semantic::analysis::component_meta::ComponentMetaAnalysis = {
        use verter_semantic::analysis::component_meta::{
            AcceptedSurfaceCompleteness, ComponentMetaAnalysis, ComponentMetaFlags,
            FallthroughSurface, NoFallthroughReason, RootReachability,
        };
        ComponentMetaAnalysis {
            props: Vec::new(),
            events: Vec::new(),
            slots: Vec::new(),
            models: Vec::new(),
            exposed: Vec::new(),
            public_instance: None,
            sfc_blocks: None,
            type_registry: Vec::new(),
            components: Vec::new(),
            template_refs: Vec::new(),
            imports: Vec::new(),
            bindings: Vec::new(),
            vue_api_calls: Vec::new(),
            styles: Vec::new(),
            flags: ComponentMetaFlags::default(),
            root_reachability: RootReachability::NoFallthrough {
                reason: NoFallthroughReason::NoTemplate,
            },
            accepted_props: Vec::new(),
            accepted_events: Vec::new(),
            accepted_surface_completeness: AcceptedSurfaceCompleteness::Exact,
            fallthrough_surface: FallthroughSurface::None {
                reason: NoFallthroughReason::NoTemplate,
            },
            macro_expansion_diagnostics: Vec::new(),
            options_api: false,
            file_path: String::new(),
        }
    };
    let cached = CachedComponentMetaResult {
        analysis,
        resolution_template: ResolutionTemplate {
            mode: crate::types::ProjectionMode::Expanded,
            whole_hash: owner_whole_hash,
            resolved_macros: Vec::new(),
            resolved_type_registry: Vec::new(),
            resolved_type_registry_meta: Vec::new(),
            evaluated_types: None,
            fact_versions: Vec::new(),
            surface_identities: None,
            origin_graph: None,
        },
        canonical_id: Arc::from(owner),
        whole_hash: owner_whole_hash,
    };
    let entry = ComponentMetaResultEntry {
        payload: Arc::new(cached),
        read_set_signature: crate::fact_signature_helpers::ReadSetSignature::empty(),
        validated_at_generation: gen0,
    };
    store
        .component_meta_results()
        .insert(key.clone(), owner_whole_hash, entry);

    let view = host.resolver_store_view();
    let db = store.component_meta_results();

    // Same generation — the carrier validates vacuously and the
    // generation matches, so `get_with_view` HITs.
    assert!(
        db.get_with_view(&host, &view, &key, owner_whole_hash)
            .is_some(),
        "an entry with a valid carrier and a matching project \
         generation must warm-hit",
    );

    // Bump ONLY the project generation (a tsconfig / SDK /
    // workspace-folder change bumps no file content) WITHOUT clearing
    // the cache. The planted entry's carrier is still valid — only
    // its `validated_at_generation` is now stale.
    store.bump_project_generation();
    assert_eq!(
        db.len(),
        1,
        "fixture invariant: bump_project_generation does NOT evict the \
         entry — the warm read must reject it on the generation stamp \
         alone",
    );

    let view_after = host.resolver_store_view();
    // DISCRIMINATOR: `get_with_view` must now MISS — the entry's
    // `validated_at_generation` no longer equals the live generation.
    // Without the generation gate `get_with_view`'s carrier check
    // alone still passes (no file content changed) and the stale
    // entry is served.
    assert!(
        db.get_with_view(&host, &view_after, &key, owner_whole_hash)
            .is_none(),
        "STALE-GENERATION READ: `ComponentMetaResultDb::get_with_view` \
         served an entry whose `validated_at_generation` is \
         superseded — a `ProjectGeneration` reset bumps no file \
         content, so the carrier check alone cannot detect it. \
         `get_with_view` must reject an entry whose generation stamp \
         no longer matches.",
    );
}

/// `SemanticGraphStore::get_relation` rejects an entry whose
/// `validated_at_generation` no longer equals the live project
/// generation — a `ProjectGeneration` reset bumps no file content, so
/// the relation carrier's `FileWholeHash`-only fact rail cannot
/// detect a project-shape change. Mirror of the `ComponentMetaResultDb`
/// test for the relation memo carrier.
#[test]
fn relation_memo_get_relation_rejects_entry_from_superseded_generation() {
    use crate::semantic_query::{PrimitiveKind, RelationResult, SemanticNodeData, SemanticNodeId};
    use crate::semantic_query_memo::SemanticGraphStore;

    let host = VerterHost::new_standalone(HostConfig::default());
    let ctx: &dyn ResolverContext = &host;
    let store = SemanticGraphStore::new();
    let source: SemanticNodeId =
        store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let target: SemanticNodeId =
        store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));

    // Plant a relation judgement with a valid (empty + empty
    // self-roots) carrier tagged at the CURRENT project generation.
    let gen0 = host.project_type_store().current_project_generation();
    store.insert_relation(
        source,
        target,
        crate::fact_signature_helpers::ReadSetSignature::empty(),
        Arc::from(Vec::<Arc<str>>::new()),
        RelationResult::NotAssignable,
        gen0,
    );

    // Same generation — the carrier validates vacuously and the
    // generation matches, so `get_relation` HITs.
    assert!(
        store.get_relation(ctx, source, target).is_some(),
        "a relation memo entry with a valid carrier and a matching \
         project generation must warm-hit",
    );

    // Bump ONLY the project generation WITHOUT clearing the relation
    // memo. The planted entry's carrier is still valid — only its
    // `validated_at_generation` is now stale.
    host.project_type_store().bump_project_generation();
    assert_eq!(
        store.relation_memo_count(),
        1,
        "fixture invariant: bump_project_generation does NOT clear \
         the relation memo — the warm read must reject the entry on \
         the generation stamp alone",
    );

    // DISCRIMINATOR: `get_relation` must now MISS — the entry's
    // `validated_at_generation` no longer equals the live generation.
    // Without the generation gate `get_relation`'s carrier check
    // alone still passes (no file content changed) and the stale
    // relation judgement is served.
    assert!(
        store.get_relation(ctx, source, target).is_none(),
        "STALE-GENERATION READ: `SemanticGraphStore::get_relation` \
         served a relation memo entry whose `validated_at_generation` \
         is superseded — a `ProjectGeneration` reset bumps no file \
         content, so the carrier's `FileWholeHash`-only rail cannot \
         detect it. `get_relation` must reject an entry whose \
         generation stamp no longer matches.",
    );
}

/// `OwnerImportSurfaceDb::get_with_view` rejects a surface whose
/// `validated_at_generation` no longer equals the live project
/// generation — a `ProjectGeneration` reset bumps no file content, so
/// the surface's chain-fact carrier cannot detect a project-shape
/// change. Mirror of the `ComponentMetaResultDb` test for the
/// owner-import-surface carrier.
#[test]
fn owner_import_surface_get_with_view_rejects_surface_from_superseded_generation() {
    use crate::owner_import_surface::build_owner_import_surface;

    let host = VerterHost::new_standalone(HostConfig::default());
    let owner = "/gen_peek_owner_import/owner.ts";
    upsert(&host, owner, "export const anchor = 1;\n");
    // Use the REAL owner whole-hash so the surface's
    // owner-`FileWholeHash` fact validates against the live store
    // view (the carrier `build_owner_import_surface` always emits an
    // owner `FileWholeHash`, so a synthetic hash would never match).
    let owner_whole_hash = host
        .ensure_indexed_ready(owner)
        .expect("owner IndexedReady materialises")
        .whole_hash;

    let surfaces = host.project_type_store().owner_import_surfaces();
    let gen0 = host.project_type_store().current_project_generation();
    let surface = build_owner_import_surface(
        Arc::from(owner),
        owner_whole_hash,
        Vec::<(Arc<str>, Arc<str>, Arc<str>, Option<crate::types::Hash16>)>::new(),
        Vec::new(),
        gen0,
    );
    surfaces.insert(Arc::from(owner), Arc::clone(&surface));

    let view = host.resolver_store_view();

    // Same generation — the carrier validates against the real owner
    // hash and the generation matches, so `get_with_view` HITs.
    assert!(
        surfaces
            .get_with_view(&host, owner, owner_whole_hash, &view)
            .is_some(),
        "an owner-import surface with a valid carrier and a matching \
         project generation must warm-hit",
    );

    // Bump ONLY the project generation WITHOUT clearing the cache.
    // The planted surface's carrier is still valid — only its
    // `validated_at_generation` is now stale.
    host.project_type_store().bump_project_generation();
    assert_eq!(
        surfaces.len(),
        1,
        "fixture invariant: bump_project_generation does NOT clear \
         the owner-import-surface cache — the warm read must reject \
         the surface on the generation stamp alone",
    );

    let view_after = host.resolver_store_view();
    // DISCRIMINATOR: `get_with_view` must now MISS — the surface's
    // `validated_at_generation` no longer equals the live generation.
    // Without the generation gate the carrier check alone still
    // passes (no file content changed) and the stale surface is
    // served.
    assert!(
        surfaces
            .get_with_view(&host, owner, owner_whole_hash, &view_after)
            .is_none(),
        "STALE-GENERATION READ: `OwnerImportSurfaceDb::get_with_view` \
         served a surface whose `validated_at_generation` is \
         superseded — a `ProjectGeneration` reset bumps no file \
         content, so the carrier check alone cannot detect it. \
         `get_with_view` must reject a surface whose generation stamp \
         no longer matches.",
    );
}
