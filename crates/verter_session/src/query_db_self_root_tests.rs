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
//! The secondary-canonical test at the end covers the `MaterializeMemoDb`
//! producer (every canonical observed during materialization is a
//! dependency fact): it primes a warm entry, edits a *secondary*
//! (non-keyed-scope) canonical through the production
//! [`crate::VerterHost::upsert`] — which performs no own-canonical
//! drain, so the entry physically survives — and asserts the warm read
//! misses. A producer that recorded no fact for the secondary canonical
//! would validate the entry stale.

use std::collections::BTreeSet;
use std::sync::Arc;

use verter_type_expr::TypeExpr;

use crate::component_meta_caches::ComputedEntry;
use crate::fact_signature_helpers::empty_fact_signature;
use crate::project_semantic_dispatch::raise::MaterializedOutputTypeExpr;
use crate::resolver_core::component_meta_query_engine::ResolvedImportedRegistrySymbol;
use crate::resolver_core::{
    FactVersionRef, MaterializeScopeObservation, ResolvedDeclarationKind, ResolvedTypeDeclaration,
    ResolverContext, StoreView,
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
            file_language: crate::LanguageRegistry::global()
                .classify_static(path)
                .static_resolution(),
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
/// the single keyed canonical it roots. The planted `MaterializeStructureDb`
/// / `RefCycleResultDb` entries carry an explicit `self_root_canonicals`
/// set; a synthetic prime passes this so the entry's strict-validation
/// set matches the planted fact.
fn planted_self_root_canonicals(canonical: &str) -> Arc<[Arc<str>]> {
    Arc::from(vec![Arc::<str>::from(canonical)])
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
    let _ = db.get_or_compute_traced_for_test(&key, ctx, || {
        ComputedEntry::Rooted(decl("stale", c), planted_self_root(c))
    });

    let mut cold_ran = false;
    let warm = db
        .get_or_compute_traced_for_test(&key, ctx, || {
            cold_ran = true;
            ComputedEntry::Rooted(decl("recomputed", c), empty_fact_signature())
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
        body: verter_type_expr::facts::PreparedTypeBodyFacts {
            classification: verter_type_expr::facts::TypeBodyClass::Alias,
            body_slot: verter_type_expr::locators::TypeBodySlot {
                anchor: verter_type_expr::locators::AuthoredAnchor {
                    canonical_id: Arc::from(canonical),
                    symbol: Arc::from(marker),
                    space: verter_type_expr::locators::LocatorSymbolSpace::Type,
                },
                path: Arc::from(Vec::new().into_boxed_slice()),
            },
            merged_contributor_slots: Arc::from(Vec::new().into_boxed_slice()),
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

    let _ = db.get_or_compute_admit_traced_for_test(&key, ctx, || {
        crate::cache_runtime::singleflight::ComputeAdmission::Cacheable(
            crate::component_meta_caches::ImportedRegistryEntry {
                value: Some(Arc::new(imported_symbol(c, "stale"))),
                fact_dep_signature: planted_self_root(c),
                validated_at_generation: ctx.project_type_store().current_project_generation(),
            },
        )
    });

    let mut cold_ran = false;
    let warm = db
        .get_or_compute_admit_traced_for_test(&key, ctx, || {
            cold_ran = true;
            crate::cache_runtime::singleflight::ComputeAdmission::Cacheable(
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

    let _ = db.get_or_compute_traced_for_test(&key, ctx, || {
        ComputedEntry::Rooted(false, planted_self_root(c))
    });

    let mut cold_ran = false;
    let warm = db
        .get_or_compute_traced_for_test(&key, ctx, || {
            cold_ran = true;
            ComputedEntry::Rooted(true, empty_fact_signature())
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

/// Marker `AuthoredBodyLocator` value for `OwnerCollectionDb` tests: the
/// marker rides the anchor SYMBOL so two publishes are distinguishable
/// while the value stays the production content-free locator shape.
fn owner_collection_marker_locator(
    canonical: &str,
    marker: &str,
) -> verter_type_expr::locators::AuthoredBodyLocator {
    verter_type_expr::locators::AuthoredBodyLocator::DeclBody(
        verter_type_expr::locators::TypeBodySlot {
            anchor: verter_type_expr::locators::AuthoredAnchor {
                canonical_id: Arc::from(canonical),
                symbol: Arc::from(marker),
                space: verter_type_expr::locators::LocatorSymbolSpace::Type,
            },
            path: Arc::from([]),
        },
    )
}

/// `OwnerCollectionDb` validates the keyed owner canonical's self-root
/// strictly. The stored locator is content-free, but the position it
/// addresses is only meaningful against the owner content version the
/// producer observed, so strict self-root validation is the
/// correctness floor.
///
/// Discriminating property: the prime attempt stores a locator carrying
/// the marker `"stale"`; the recompute stores `"recomputed"`. A lazy
/// validator admits the stale locator and the warm read returns it; the
/// strict validator rejects admission.
#[test]
fn owner_collection_db_untracked_self_root_rejects_warm_entry() {
    let host = host_with_unrelated_file();
    let c = "/self_root_qdb/owner_never_loaded.ts";
    assert_untracked(&host, c);
    let ctx: &dyn ResolverContext = &host;
    let db = host.project_type_store().owner_collection_db();
    let key = (Arc::<str>::from(c), Arc::<str>::from("Probe"));

    let _ = db.get_or_compute_traced_for_test(&key, ctx, || {
        ComputedEntry::Rooted(
            Some(owner_collection_marker_locator(c, "stale")),
            planted_self_root(c),
        )
    });

    let mut cold_ran = false;
    let warm = db
        .get_or_compute_traced_for_test(&key, ctx, || {
            cold_ran = true;
            ComputedEntry::Rooted(
                Some(owner_collection_marker_locator(c, "recomputed")),
                empty_fact_signature(),
            )
        })
        .expect("warm path produces a value");

    assert!(
        cold_ran,
        "OwnerCollectionDb MUST NOT serve a warm entry whose self-root names an \
         untracked keyed canonical",
    );
    assert_eq!(
        warm,
        Some(owner_collection_marker_locator(c, "recomputed")),
        "the rejected entry must not bubble its stale locator",
    );
}

// ---------------------------------------------------------------------------
// Item 10 — MaterializeMemoDb.
// ---------------------------------------------------------------------------

fn materialized(
    marker: &str,
    dep_signature: crate::semantic_query::DepSignature,
) -> MaterializedOutputTypeExpr {
    MaterializedOutputTypeExpr::from_type_expr_for_test(
        None,
        TypeExpr::Unknown {
            raw: marker.to_string(),
        },
        dep_signature,
        false,
    )
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
    use crate::semantic_query::{ProjectionMode, SemanticNodeId};

    let host = host_with_unrelated_file();
    let c = "/self_root_qdb/memo_never_loaded.ts";
    assert_untracked(&host, c);
    let ctx: &dyn ResolverContext = &host;
    let db = host.project_type_store().shape_cache_db();
    // The TypeExpr-start route keys its LOWERED settled node; this
    // cache-rail fixture mints an arbitrary test node in the same
    // member-value subject class production keys.
    let key = crate::component_meta_caches::ShapeCacheKey::member_value_node_whole_for_test(
        Arc::<str>::from(c),
        SemanticNodeId(7101),
        ProjectionMode::Expanded,
    );

    let _ = db.get_or_compute_traced_for_test(&key, ctx, || {
        Some((
            materialized("stale", empty_dep_signature()),
            planted_self_root(c),
        ))
    });

    let mut cold_ran = false;
    let warm = db
        .get_or_compute_traced_for_test(&key, ctx, || {
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
        matches!(&warm.type_expr_for_test(), TypeExpr::Unknown { raw } if raw == "recomputed"),
        "the rejected entry must not bubble its stale materialized expression",
    );
}

/// Universal `ShapeCacheDb` (MemberValueNode
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
fn member_value_node_cache_db_untracked_self_root_rejects_warm_entry() {
    use crate::semantic_query::{ProjectionMode, SemanticNodeId};

    let host = host_with_unrelated_file();
    let c = "/self_root_qdb/member_shape_never_loaded.ts";
    assert_untracked(&host, c);
    let ctx: &dyn ResolverContext = &host;
    let db = host.project_type_store().shape_cache_db();
    // Use a synthetic SemanticNodeId — the test exercises the cache's
    // self-root validation contract, not the production graph (the
    // arbitrary-node test-only constructor, not the member-value path).
    let key = crate::component_meta_caches::ShapeCacheKey::member_value_node_whole_for_test(
        Arc::<str>::from(c),
        SemanticNodeId(7),
        ProjectionMode::Expanded,
    );

    let _ = db.get_or_compute_traced_for_test(&key, ctx, || {
        Some((
            materialized("stale", empty_dep_signature()),
            planted_self_root(c),
        ))
    });

    let mut cold_ran = false;
    let warm = db
        .get_or_compute_traced_for_test(&key, ctx, || {
            cold_ran = true;
            Some((
                materialized("recomputed", empty_dep_signature()),
                empty_fact_signature(),
            ))
        })
        .expect("warm path produces a value");

    assert!(
        cold_ran,
        "ShapeCacheDb (MemberValueNode subject) MUST NOT serve a warm entry whose \
         self-root names an untracked keyed canonical",
    );
    assert!(
        matches!(&warm.type_expr_for_test(), TypeExpr::Unknown { raw } if raw == "recomputed"),
        "the rejected entry must not bubble its stale materialized expression",
    );
}

/// Universal `ShapeCacheDb` (SyntheticBinding subject) validates the
/// keyed scope canonical's self-root strictly through the SAME
/// fact-signature rail as the `TypeExpr` and `SemanticNode` subjects.
///
/// The synthetic-binding subject is the content-free
/// [`crate::semantic_query::SyntheticBindingId`] identity — the cache key
/// carries no `value_node` ordinal. Correctness of a single-entry cache
/// under a content-free key holds ONLY through the rail: a warm entry
/// whose self-root names an untracked / changed keyed canonical must be
/// recomputed cold, never stale-served.
///
/// Discriminating property: the prime attempt's materialized expression
/// carries the marker `"stale"`; the recompute carries `"recomputed"`. A
/// lazy validator admits the stale expression and the second
/// `get_or_compute` returns it; the strict validator rejects admission
/// for an untracked keyed canonical, so the second call's cold closure
/// runs and the recomputed value surfaces.
///
/// Mirrors `member_value_node_cache_db_untracked_self_root_rejects_warm_entry`
/// exactly — every subject shares the
/// `cooperative_get_or_insert_with_post_publish` + fact-signature
/// self-root contract.
#[test]
fn shape_cache_db_synthetic_binding_untracked_self_root_rejects_warm_entry() {
    use crate::semantic_query::{ProjectionMode, SyntheticBindingId};
    use verter_type_expr::{SyntheticCarrierKey, SyntheticCarrierSurfaceKind};

    let host = host_with_unrelated_file();
    let c = "/self_root_qdb/synthetic_binding_never_loaded.ts";
    assert_untracked(&host, c);
    let ctx: &dyn ResolverContext = &host;
    let db = host.project_type_store().shape_cache_db();

    // Content-free synthetic-binding identity rooted on the untracked
    // keyed canonical. The `value_node` is value-side provenance only —
    // the identity drops it, so the key roots on `c` via
    // `scope_canonical_id`.
    let carrier = SyntheticCarrierKey {
        scope_canonical_id: Arc::<str>::from(c),
        surface_kind: SyntheticCarrierSurfaceKind::SlotBinding,
        slot_name: Some(Arc::from("default")),
        binding_name: Arc::from("items"),
        value_node: 7,
    };
    let key = crate::component_meta_caches::ShapeCacheKey::synthetic_binding_whole(
        SyntheticBindingId::from_carrier_key(&carrier),
        ProjectionMode::Expanded,
    );

    let _ = db.get_or_compute_traced_for_test(&key, ctx, || {
        Some((
            materialized("stale", empty_dep_signature()),
            planted_self_root(c),
        ))
    });

    let mut cold_ran = false;
    let warm = db
        .get_or_compute_traced_for_test(&key, ctx, || {
            cold_ran = true;
            Some((
                materialized("recomputed", empty_dep_signature()),
                empty_fact_signature(),
            ))
        })
        .expect("warm path produces a value");

    assert!(
        cold_ran,
        "ShapeCacheDb (SyntheticBinding subject) MUST NOT serve a warm entry \
         whose self-root names an untracked keyed canonical",
    );
    assert!(
        matches!(&warm.type_expr_for_test(), TypeExpr::Unknown { raw } if raw == "recomputed"),
        "the rejected entry must not bubble its stale materialized expression",
    );

    // A second carrier with a DIFFERENT `value_node` but the same
    // content-free identity keys the SAME slot — the rail-validated entry
    // does not split per provenance ordinal.
    let carrier_other_node = SyntheticCarrierKey {
        value_node: 99,
        ..carrier.clone()
    };
    let key_other_node = crate::component_meta_caches::ShapeCacheKey::synthetic_binding_whole(
        SyntheticBindingId::from_carrier_key(&carrier_other_node),
        ProjectionMode::Expanded,
    );
    assert_eq!(
        key, key_other_node,
        "two carriers differing only in `value_node` must produce the SAME \
         content-free cache key — the ordinal is provenance, not identity",
    );
}

/// `MaterializeMemoDb`'s producer records a dependency `FileWholeHash`
/// for every canonical the materialization walk observed. A content
/// edit to an observed dependency invalidates the memo even though the
/// keyed scope canonical is unchanged.
///
/// Discriminating property: the entry is keyed on `scope` but its
/// `MaterializedOutputTypeExpr.dep_signature` lists an observed dependency
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
    use crate::semantic_query::{DepVersion, ProjectionMode, SemanticNodeId};

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
    // The TypeExpr-start route keys its LOWERED settled node; this
    // cache-rail fixture mints an arbitrary test node in the same
    // member-value subject class production keys.
    let key = crate::component_meta_caches::ShapeCacheKey::member_value_node_whole_for_test(
        Arc::<str>::from(scope),
        SemanticNodeId(7102),
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
        .get_or_compute_traced_for_test(&key, ctx, move || {
            // `dep` is recorded as `DepVersion::WholeHash`, so the
            // signature builder returns `Some` and the entry is
            // admitted.
            let export_set = observed_scope.syntactic_export_set.clone()?;
            let sig = engine_fact_signature_for_materialize_memo(
                &observed_scope,
                export_set,
                &primed_dep_sig,
            )
            .into_cacheable()?
            .facts;
            Some((materialized("stale", Arc::clone(&primed_dep_sig)), sig))
        })
        .expect("cold publish succeeds");
    assert!(
        matches!(&primed.type_expr_for_test(), TypeExpr::Unknown { raw } if raw == "stale"),
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
        .get_or_compute_traced_for_test(&key, ctx2, || {
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
        matches!(&warm.type_expr_for_test(), TypeExpr::Unknown { raw } if raw == "recomputed"),
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
//     `_for_canonical_member` / `_for_materialize_memo`).
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
        .get_or_compute_traced_for_test(&key, ctx, || {
            let sig = engine_fact_signature_for_exported_type(
                ctx,
                owned.as_str(),
                "Probe",
                observed_keyed_hash,
            )
            .into_cacheable()
            .expect("provenance-pure signature builds — observed artifact present")
            .facts;
            ComputedEntry::Rooted(decl("stale", c), sig)
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
        .get_or_compute_traced_for_test(&key, ctx2, || {
            cold_ran = true;
            ComputedEntry::Rooted(decl("recomputed", c), empty_fact_signature())
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
        .get_or_compute_admit_traced_for_test(&key, ctx, || {
            let sig = engine_fact_signature_for_exported_type(
                ctx,
                owned.as_str(),
                "Probe",
                observed_keyed_hash,
            )
            .into_cacheable()
            .expect("provenance-pure signature builds — observed artifact present")
            .facts;
            crate::cache_runtime::singleflight::ComputeAdmission::Cacheable(
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
        .get_or_compute_admit_traced_for_test(&key, ctx2, || {
            cold_ran = true;
            crate::cache_runtime::singleflight::ComputeAdmission::Cacheable(
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
        .get_or_compute_traced_for_test(&key, ctx, || {
            let sig = engine_fact_signature_for_exported_type(
                ctx,
                owned.as_str(),
                "Probe",
                observed_keyed_hash,
            )
            .into_cacheable()
            .expect("provenance-pure signature builds — observed artifact present")
            .facts;
            ComputedEntry::Rooted(false, sig)
        })
        .expect("cold publish succeeds");

    sibling_body_edit(&host, c);

    let ctx2: &dyn ResolverContext = &host;
    let mut cold_ran = false;
    let warm = db
        .get_or_compute_traced_for_test(&key, ctx2, || {
            cold_ran = true;
            ComputedEntry::Rooted(true, empty_fact_signature())
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
/// (called by `owner_collection_expr`); the stored locator only
/// addresses the observed owner content version, so the self-root
/// `FileWholeHash` is the correctness floor. An unrelated-sibling edit
/// shifts only the self-root. Verified: neutering `self_root_fact`
/// flips this canary RED.
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
        .get_or_compute_traced_for_test(&key, ctx, || {
            let sig = engine_fact_signature_for_exported_type(
                ctx,
                owned.as_str(),
                "Probe",
                observed_keyed_hash,
            )
            .into_cacheable()
            .expect("provenance-pure signature builds — observed artifact present")
            .facts;
            ComputedEntry::Rooted(Some(owner_collection_marker_locator(c, "stale")), sig)
        })
        .expect("cold publish succeeds");

    sibling_body_edit(&host, c);

    let ctx2: &dyn ResolverContext = &host;
    let mut cold_ran = false;
    let warm = db
        .get_or_compute_traced_for_test(&key, ctx2, || {
            cold_ran = true;
            ComputedEntry::Rooted(
                Some(owner_collection_marker_locator(c, "recomputed")),
                empty_fact_signature(),
            )
        })
        .expect("warm path produces a value");

    assert!(
        cold_ran,
        "OwnerCollectionDb warm read MUST reject the entry after an unrelated-sibling \
         edit to its keyed canonical — only the producer's self-root FileWholeHash \
         catches it. Reverting the self_root_fact prepend serves stale.",
    );
    assert_eq!(
        warm,
        Some(owner_collection_marker_locator(c, "recomputed")),
        "the rejected warm entry must not bubble its stale locator",
    );
}

/// `OwnerCollectionDb` reuse + `invalidate_canonical` acceptance, asserted
/// through the `live_count()` entry-count handle (the count is `entries.len()`).
/// The sibling `owner_collection_db_self_root_sibling_edit_rejects_warm_entry`
/// asserts self-root rejection via a `cold_ran` flag after a content edit; this
/// pairs the cache-hit-EQUIVALENCE half (a warm re-read REUSES the entry — no
/// recompute, `live_count` stable) with the explicit `invalidate_canonical`
/// invalidation path, both read through `live_count()`.
///
/// 1. Populate: a cold publish over a TRACKED owner admits exactly one entry —
///    `live_count()` goes 0 → 1.
/// 2. Reuse equivalence: a second unchanged `get_or_compute` REUSES the warm
///    entry — its cold closure does NOT run and `live_count()` stays 1.
/// 3. Invalidation: `invalidate_canonical(owner)` drops the owner's entry —
///    `live_count()` goes 1 → 0 — and the next `get_or_compute` RECOMPUTES
///    (cold closure runs) and re-admits the entry (`live_count()` back to 1).
///
/// Discriminates: if warm reuse regressed, step 2's cold closure runs and
/// `live_count` would have to climb to admit a second entry; if
/// `invalidate_canonical` regressed (did not drop the owner's entry),
/// `live_count` stays 1 across the invalidation and the post-invalidation read
/// is a stale warm hit (cold closure does NOT run).
#[test]
fn owner_collection_db_reuses_warm_then_invalidate_canonical_drops_and_recomputes() {
    use crate::resolver_core::component_meta_query_engine::engine_fact_signature_for_exported_type;

    let host = VerterHost::new_standalone(HostConfig::default());
    let c = "/self_root_owner_collection/owner.ts";
    load_tracked_keyed(&host, c);
    let ctx: &dyn ResolverContext = &host;
    let db = host.project_type_store().owner_collection_db();
    let key = (Arc::<str>::from(c), Arc::<str>::from("Probe"));

    assert_eq!(
        db.live_count(),
        0,
        "fixture invariant: the OwnerCollectionDb starts with no entry for the owner"
    );

    // Observe the keyed canonical's content version at cold-publish time,
    // exactly as the production producer does.
    let observed_keyed_hash = observed_whole_hash(ctx, c);
    let owned = c.to_string();
    let publish = |marker: &'static str| {
        let owned = owned.clone();
        move || {
            let sig = engine_fact_signature_for_exported_type(
                ctx,
                owned.as_str(),
                "Probe",
                observed_keyed_hash,
            )
            .into_cacheable()
            .expect("provenance-pure signature builds — observed artifact present")
            .facts;
            ComputedEntry::Rooted(
                Some(owner_collection_marker_locator(owned.as_str(), marker)),
                sig,
            )
        }
    };

    // ── Populate: cold publish admits exactly one entry (live_count 0 -> 1).
    let _ = db
        .get_or_compute_traced_for_test(&key, ctx, publish("first"))
        .expect("cold publish succeeds");
    assert_eq!(
        db.live_count(),
        1,
        "populate: a cold publish over a TRACKED owner must admit exactly one entry"
    );

    // ── Reuse equivalence: an unchanged second read REUSES the warm entry —
    // its cold closure must NOT run and live_count must stay 1.
    let mut reuse_cold_ran = false;
    let warm = db
        .get_or_compute_traced_for_test(&key, ctx, || {
            reuse_cold_ran = true;
            ComputedEntry::Rooted(
                Some(owner_collection_marker_locator(c, "should-not-run")),
                empty_fact_signature(),
            )
        })
        .expect("warm read produces a value");
    assert!(
        !reuse_cold_ran,
        "cache-hit equivalence: an unchanged second get_or_compute MUST reuse the warm \
         OwnerCollectionDb entry — its cold closure must NOT run"
    );
    assert_eq!(
        db.live_count(),
        1,
        "cache-hit equivalence: a warm reuse must NOT admit a second entry (live_count stable at 1)"
    );
    assert_eq!(
        warm,
        Some(owner_collection_marker_locator(c, "first")),
        "the warm reuse must bubble the originally-published locator, not recompute"
    );

    // ── Invalidation: invalidate_canonical drops the owner's entry
    // (live_count 1 -> 0).
    db.invalidate_canonical(c);
    assert_eq!(
        db.live_count(),
        0,
        "DISCRIMINATING (invalidation): invalidate_canonical MUST drop the owner's \
         OwnerCollectionDb entry (live_count 1 -> 0) — a retained entry would serve stale"
    );

    // ── Recompute: the next read RECOMPUTES (cold closure runs) and re-admits.
    // The flag is a `Cell` captured by reference (so the `move` closure, which
    // must own `inner`, moves the REFERENCE and mutates the SAME flag rather
    // than a copied `bool`).
    let recompute_cold_ran = std::cell::Cell::new(false);
    let recompute_flag = &recompute_cold_ran;
    let recomputed = db
        .get_or_compute_traced_for_test(&key, ctx, {
            let inner = publish("second");
            move || {
                recompute_flag.set(true);
                inner()
            }
        })
        .expect("post-invalidation read produces a value");
    assert!(
        recompute_cold_ran.get(),
        "DISCRIMINATING (invalidation): after invalidate_canonical the next read MUST \
         recompute (cold closure runs), not serve a stale warm entry"
    );
    assert_eq!(
        db.live_count(),
        1,
        "the recompute must re-admit exactly one entry (live_count back to 1)"
    );
    assert_eq!(
        recomputed,
        Some(owner_collection_marker_locator(c, "second")),
        "the recompute must surface the freshly-published locator"
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
    use crate::semantic_query::{ProjectionMode, SemanticNodeId};

    let host = VerterHost::new_standalone(HostConfig::default());
    let c = "/self_root_e2e/memo.ts";
    load_tracked_keyed(&host, c);
    let ctx: &dyn ResolverContext = &host;
    // The single tear-free scope observation taken at
    // materialisation/cold-publish time — `observe_materialize_scope`
    // pins the scope's current `IndexedReady`.
    let db = host.project_type_store().shape_cache_db();
    // The TypeExpr-start route keys its LOWERED settled node; this
    // cache-rail fixture mints an arbitrary test node in the same
    // member-value subject class production keys.
    let key = crate::component_meta_caches::ShapeCacheKey::member_value_node_whole_for_test(
        Arc::<str>::from(c),
        SemanticNodeId(7103),
        ProjectionMode::Expanded,
    );

    let observed_scope = observe_scope(ctx, c);
    let _ = db
        .get_or_compute_traced_for_test(&key, ctx, || {
            // No observed dependencies — the signature builder returns
            // `Some` (no `RouteGeneration` entry) and the discriminator
            // is the scope canonical's own self-root `FileWholeHash`,
            // rooted on the observation's content version.
            let export_set = observed_scope.syntactic_export_set.clone()?;
            let sig = engine_fact_signature_for_materialize_memo(
                &observed_scope,
                export_set,
                &empty_dep_signature(),
            )
            .into_cacheable()?
            .facts;
            Some((materialized("stale", empty_dep_signature()), sig))
        })
        .expect("cold publish succeeds");

    sibling_body_edit(&host, c);

    let ctx2: &dyn ResolverContext = &host;
    let mut cold_ran = false;
    let warm = db
        .get_or_compute_traced_for_test(&key, ctx2, || {
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
        matches!(&warm.type_expr_for_test(), TypeExpr::Unknown { raw } if raw == "recomputed"),
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
/// `MaterializedOutputTypeExpr` is still returned to the caller; only the
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
    use crate::semantic_query::{DepVersion, ProjectionMode, SemanticNodeId};

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
    // The TypeExpr-start route keys its LOWERED settled node; this
    // cache-rail fixture mints an arbitrary test node in the same
    // member-value subject class production keys.
    let key = crate::component_meta_caches::ShapeCacheKey::member_value_node_whole_for_test(
        Arc::<str>::from(scope),
        SemanticNodeId(7104),
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
        sig.cacheable().is_none(),
        "engine_fact_signature_for_materialize_memo MUST refuse admission (NonCacheable) when an observed \
         dependency carries DepVersion::RouteGeneration — route generation has no \
         validating source, so the entry must not be admitted to the shared memo. A \
         producer that roots the RouteGeneration dependency by any fact returns \
         Cacheable and admits the entry; that admission is the unsoundness this refusal \
         closes.",
    );

    // Drive the publish path exactly as the production write-through
    // does: the closure threads the `None` signature through `?`, so
    // `get_or_compute`'s compute returns `None` and nothing is
    // admitted.
    let primed_dep_sig = Arc::clone(&dep_sig);
    let cold_value = db.get_or_compute_traced_for_test(&key, ctx, move || {
        let export_set = observed_scope.syntactic_export_set.clone()?;
        let fact_sig = engine_fact_signature_for_materialize_memo(
            &observed_scope,
            export_set,
            &primed_dep_sig,
        )
        .into_cacheable()?
        .facts;
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
        .get_or_compute_traced_for_test(&key, ctx2, || {
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
        matches!(&value.type_expr_for_test(), TypeExpr::Unknown { raw } if raw == "recomputed"),
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
    use crate::semantic_query::{DepVersion, ProjectionMode, SemanticNodeId};

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
    // The TypeExpr-start route keys its LOWERED settled node; this
    // cache-rail fixture mints an arbitrary test node in the same
    // member-value subject class production keys.
    let key = crate::component_meta_caches::ShapeCacheKey::member_value_node_whole_for_test(
        Arc::<str>::from(scope),
        SemanticNodeId(7105),
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
    .into_cacheable()
    .expect("a ProjectGeneration dep signature is admissible (Some)")
    .facts;
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
        .get_or_compute_traced_for_test(&key, ctx, move || {
            let export_set = observed_scope.syntactic_export_set.clone()?;
            let fact_sig = engine_fact_signature_for_materialize_memo(
                &observed_scope,
                export_set,
                &primed_dep_sig,
            )
            .into_cacheable()?
            .facts;
            Some((materialized("stale", Arc::clone(&primed_dep_sig)), fact_sig))
        })
        .expect("cold publish succeeds");
    assert!(
        matches!(&primed.type_expr_for_test(), TypeExpr::Unknown { raw } if raw == "stale"),
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
        .get_or_compute_traced_for_test(&key, ctx2, || {
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
        matches!(&warm.type_expr_for_test(), TypeExpr::Unknown { raw } if raw == "recomputed"),
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
///   The stale `MaterializedOutputTypeExpr` would then be published rooted by
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
    .into_cacheable()
    .expect("a WholeHash-only dep signature must produce an admissible fact signature")
    .facts;

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
         would publish the stale MaterializedOutputTypeExpr rooted by a fresh-looking hash \
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
    .into_cacheable()
    .expect("a dep-free materialize-memo signature is admissible")
    .facts;

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
         MaterializedOutputTypeExpr rooted by a fresh-looking hash that validates on every warm \
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
    use crate::semantic_query::{ProjectionMode, SemanticNodeId};

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
    // The TypeExpr-start route keys its LOWERED settled node; this
    // cache-rail fixture mints an arbitrary test node in the same
    // member-value subject class production keys.
    let key = crate::component_meta_caches::ShapeCacheKey::member_value_node_whole_for_test(
        Arc::<str>::from(scope),
        SemanticNodeId(7106),
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
    let cold_value = db.get_or_compute_traced_for_test(&key, ctx, move || {
        let export_set = observed_scope_h1.syntactic_export_set.clone()?;
        let fact_sig = engine_fact_signature_for_materialize_memo(
            &observed_scope_h1,
            export_set,
            &empty_dep_signature(),
        )
        .into_cacheable()?
        .facts;
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
        .get_or_compute_traced_for_test(&key, ctx2, || {
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
        matches!(&value.type_expr_for_test(), TypeExpr::Unknown { raw } if raw == "recomputed"),
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
        sig.cacheable().is_none(),
        "engine_fact_signature_for_materialize_memo MUST refuse admission (NonCacheable) when an observed \
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
        sig.cacheable().is_none(),
        "engine_fact_signature_for_materialize_memo MUST refuse admission (NonCacheable) when the supplied \
         SyntacticExportSet parse fact describes a canonical other than the keyed scope — \
         a Parse fact for the wrong file mis-roots the entry. A builder that pushes the \
         supplied parse fact without a canonical-equality guard emits a self-root \
         signature describing two different files.",
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
        .into_cacheable()
        .expect("observed-current signature builds")
        .facts;
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
        engine_fact_signature_for_exported_type(ctx2, c, "Probe", observed_h1)
            .into_cacheable()
            .is_none(),
        "ImportedRegistryDb's signature builder MUST return None for the STALE observed \
         hash H1 after the keyed canonical was edited to H2 — the H1 parse-fact registry \
         is drained, so shared-cache admission is refused. A pre-fix builder re-reads \
         authoritative_current_content_hash, resolves H2's registry, and returns Some \
         rooted on H2.",
    );

    // Step 5 — the CURRENT observed hash still builds, proving step 4
    // is the stale-observation refusal, not a broken builder.
    let current_sig = engine_fact_signature_for_exported_type(ctx2, c, "Probe", current_h2)
        .into_cacheable()
        .expect("current-observed signature still builds")
        .facts;
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
        .into_cacheable()
        .expect("observed-current signature builds")
        .facts;
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
        engine_fact_signature_for_exported_type(ctx2, c, "Probe", observed_h1)
            .into_cacheable()
            .is_none(),
        "DeclarationLookupDb's signature builder MUST return None for the STALE observed \
         hash H1 after the keyed canonical was edited to H2. A pre-fix builder re-reads \
         current content and returns Some rooted on H2.",
    );

    let current_sig = engine_fact_signature_for_exported_type(ctx2, c, "Probe", current_h2)
        .into_cacheable()
        .expect("current-observed signature still builds")
        .facts;
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
        .into_cacheable()
        .expect("observed-current signature builds")
        .facts;
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
        engine_fact_signature_for_exported_type(ctx2, c, "Probe", observed_h1)
            .into_cacheable()
            .is_none(),
        "ResolvabilityDb's signature builder MUST return None for the STALE observed \
         hash H1 after the keyed canonical was edited to H2. A pre-fix builder re-reads \
         current content and returns Some rooted on H2.",
    );

    let current_sig = engine_fact_signature_for_exported_type(ctx2, c, "Probe", current_h2)
        .into_cacheable()
        .expect("current-observed signature still builds")
        .facts;
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
        .into_cacheable()
        .expect("observed-current signature builds")
        .facts;
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
        engine_fact_signature_for_exported_type(ctx2, c, "Probe", observed_h1)
            .into_cacheable()
            .is_none(),
        "OwnerCollectionDb's signature builder MUST return None for the STALE observed \
         hash H1 after the keyed canonical was edited to H2. A pre-fix builder re-reads \
         current content and returns Some rooted on H2.",
    );

    let current_sig = engine_fact_signature_for_exported_type(ctx2, c, "Probe", current_h2)
        .into_cacheable()
        .expect("current-observed signature still builds")
        .facts;
    assert!(
        signature_roots_whole_hash(&current_sig, c, current_h2),
        "the current-observed signature must root on H2",
    );
}

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
        .resolver_store_view_read()
        .into_owned_view()
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
        .resolver_store_view_read()
        .into_owned_view()
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
        .resolver_store_view_read()
        .into_owned_view()
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
    use crate::semantic_query::{ProjectionMode, SemanticNodeId};

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
    // The TypeExpr-start route keys its LOWERED settled node; this
    // cache-rail fixture mints an arbitrary test node in the same
    // member-value subject class production keys.
    let key = crate::component_meta_caches::ShapeCacheKey::member_value_node_whole_for_test(
        Arc::<str>::from(scope),
        SemanticNodeId(7107),
        ProjectionMode::Expanded,
    );
    let scope_owned = scope.to_string();
    let cold_value = db.get_or_compute_traced_for_test(&key, ctx, move || {
        // The production publish closure: a `None` observation refuses
        // shared-cache admission.
        let observed_scope = ctx.observe_materialize_scope(scope_owned.as_str())?;
        let export_set = observed_scope.syntactic_export_set.clone()?;
        let fact_sig = engine_fact_signature_for_materialize_memo(
            &observed_scope,
            export_set,
            &empty_dep_signature(),
        )
        .into_cacheable()?
        .facts;
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
        .get_or_compute_traced_for_test(&key, ctx2, || {
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
        matches!(&value.type_expr_for_test(), TypeExpr::Unknown { raw } if raw == "recomputed"),
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

// ---------------------------------------------------------------------------
// Cross-view candidate isolation through `ImportedRegistryDb` (R20).
// ---------------------------------------------------------------------------
//
// `ImportedRegistryDb::get_or_compute_admit` routes through the
// query-identity split-publish lifecycle over the shared
// `ReverseIndexedCandidateStore`. The flight lane is keyed by
// `(key, store-view compat token)`, so a base request and an overlay
// request on the SAME key run on DISTINCT flight lanes — they do NOT
// coalesce onto one cold build. Each computes and admits its OWN
// candidate, and the two coexist in one content-free slot (R20 overlay
// isolation). A reader under either view selects the candidate that
// validates against its own content identity; the other view's candidate
// is never served cross-view.

/// A base request and an overlay request on the same imported-registry
/// key each resolve their OWN view-accurate symbol, and the two
/// candidates COEXIST in the slot — neither overwrites the other (R20).
#[test]
fn imported_registry_base_and_overlay_candidates_coexist() {
    use crate::resolver_core::SessionResolverContext;
    use crate::session_view::{OverlaidView, SessionView};
    use rustc_hash::FxHashMap;

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
    let key: (Arc<str>, Arc<str>) = (Arc::<str>::from(canonical), Arc::<str>::from("Probe"));

    // Base-view publish — self-roots the keyed canonical at its BASE hash.
    let base_self_root: Arc<[FactVersionRef]> = Arc::from(vec![FactVersionRef::FileWholeHash {
        canonical_id: canonical.to_string(),
        hash: base_hash,
    }]);
    {
        let ctx: &dyn ResolverContext = host.as_ref();
        let db = host.project_type_store().imported_registry_db();
        let base_cold_ran = std::cell::Cell::new(false);
        let resolved = db
            .get_or_compute_admit_traced_for_test(&key, ctx, || {
                base_cold_ran.set(true);
                crate::cache_runtime::singleflight::ComputeAdmission::Cacheable(
                    crate::component_meta_caches::ImportedRegistryEntry {
                        value: Some(Arc::new(imported_symbol(canonical, "winner-base"))),
                        fact_dep_signature: Arc::clone(&base_self_root),
                        validated_at_generation: ctx
                            .project_type_store()
                            .current_project_generation(),
                    },
                )
            })
            .flatten();
        assert!(base_cold_ran.get(), "the base view cold-computes");
        assert_eq!(
            resolved.map(|s| s.exported_name.clone()),
            Some("winner-base".to_string()),
            "the base view resolves its own base symbol",
        );
    }

    // Overlay-view publish — re-roots the keyed canonical to a DIFFERENT
    // content hash. A different flight lane (distinct compat token), so it
    // does NOT coalesce onto the base candidate; it cold-computes its own.
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
    {
        let session_store_view = host
            .resolver_store_view_read()
            .into_owned_view()
            .with_session_overlay(&host, &view);
        let session_ctx = SessionResolverContext::new(
            &host,
            &view,
            &session_store_view,
            std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new()),
        );
        let db = host.project_type_store().imported_registry_db();
        let overlay_cold_ran = std::cell::Cell::new(false);
        let resolved = db
            .get_or_compute_admit_traced_for_test(&key, &session_ctx, || {
                overlay_cold_ran.set(true);
                crate::cache_runtime::singleflight::ComputeAdmission::Cacheable(
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
            .flatten();
        // DISCRIMINATOR: the overlay request must NOT inherit the base
        // candidate — its base-rooted self-root mismatches the overlay
        // hash, so the overlay cold-computes its OWN symbol.
        assert!(
            overlay_cold_ran.get(),
            "the overlay view MUST cold-compute its own candidate — the base \
             candidate self-roots the keyed canonical at the BASE hash and is \
             not interchangeable across views",
        );
        assert_eq!(
            resolved.map(|s| s.exported_name.clone()),
            Some("follower-overlay".to_string()),
            "the overlay view resolves its OWN overlay symbol",
        );
    }

    // R20 COEXISTENCE: both the base candidate and the overlay candidate
    // are live in the one content-free slot — neither overwrote the other.
    let db = host.project_type_store().imported_registry_db();
    assert_eq!(
        db.live_count(),
        2,
        "the base and overlay candidates MUST COEXIST as two distinct \
         candidates in one content-free slot (R20 overlay isolation) — a \
         cap-1 / always-replace store would have let the overlay publish \
         clobber the base candidate, leaving one",
    );
    // The base reader still sees its own symbol (the overlay candidate did
    // not displace it).
    let ctx: &dyn ResolverContext = host.as_ref();
    let base_again = db.peek(&key, ctx).flatten();
    assert_eq!(
        base_again.map(|s| s.exported_name.clone()),
        Some("winner-base".to_string()),
        "after the overlay published, a base-view peek still resolves the \
         base candidate — the two coexist",
    );
}

/// The shared `component_meta_cache_live` counter delta equals
/// `ImportedRegistryDb`'s live-candidate delta after a base + overlay
/// publish: every admitted candidate contributes exactly one counter
/// increment, and nothing double-counts or under-counts.
#[test]
fn imported_registry_coexisting_candidates_keep_live_counter_consistent() {
    use crate::resolver_core::SessionResolverContext;
    use crate::session_view::{OverlaidView, SessionView};
    use rustc_hash::FxHashMap;
    use std::sync::atomic::Ordering;

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
    let key: (Arc<str>, Arc<str>) = (Arc::<str>::from(canonical), Arc::<str>::from("Probe"));

    let live_counter = Arc::clone(&host.project_type_store().counters.component_meta_cache_live);
    let counter_before = live_counter.load(Ordering::Relaxed);
    let entries_before = host
        .project_type_store()
        .imported_registry_db()
        .live_count();

    // Base publish.
    let base_self_root: Arc<[FactVersionRef]> = Arc::from(vec![FactVersionRef::FileWholeHash {
        canonical_id: canonical.to_string(),
        hash: base_hash,
    }]);
    {
        let ctx: &dyn ResolverContext = host.as_ref();
        let db = host.project_type_store().imported_registry_db();
        let _ = db.get_or_compute_admit_traced_for_test(&key, ctx, || {
            crate::cache_runtime::singleflight::ComputeAdmission::Cacheable(
                crate::component_meta_caches::ImportedRegistryEntry {
                    value: Some(Arc::new(imported_symbol(canonical, "winner-base"))),
                    fact_dep_signature: Arc::clone(&base_self_root),
                    validated_at_generation: ctx.project_type_store().current_project_generation(),
                },
            )
        });
    }

    // Overlay publish (distinct flight lane → distinct candidate).
    let overlay_source: Arc<str> = Arc::from("export interface Probe { overlaid: string; }\n");
    let mut overlays: FxHashMap<String, Arc<str>> = FxHashMap::default();
    overlays.insert(canonical.to_string(), Arc::clone(&overlay_source));
    let view = OverlaidView::new(Arc::clone(&host), overlays);
    let overlay_hash = view
        .overlay_content_hash_for(canonical)
        .expect("overlay content hash present");
    host.materialize_overlay_indexed_ready_with_view(canonical, &view)
        .expect("overlay IndexedReady materialises");
    {
        let session_store_view = host
            .resolver_store_view_read()
            .into_owned_view()
            .with_session_overlay(&host, &view);
        let session_ctx = SessionResolverContext::new(
            &host,
            &view,
            &session_store_view,
            std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new()),
        );
        let db = host.project_type_store().imported_registry_db();
        let _ = db.get_or_compute_admit_traced_for_test(&key, &session_ctx, || {
            crate::cache_runtime::singleflight::ComputeAdmission::Cacheable(
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
        });
    }

    let db = host.project_type_store().imported_registry_db();
    let counter_after = live_counter.load(Ordering::Relaxed);
    let entries_after = db.live_count();
    let counter_delta: u64 = counter_after - counter_before;
    let entries_delta: u64 = (entries_after - entries_before) as u64;

    // Two coexisting candidates — the counter delta and the live-candidate
    // delta must both be 2 and equal.
    assert_eq!(
        entries_delta, 2,
        "the base and overlay candidates coexist — two live candidates",
    );
    assert_eq!(
        counter_delta, entries_delta,
        "the shared `component_meta_cache_live` counter delta ({counter_delta}) \
         MUST equal `ImportedRegistryDb`'s live-candidate delta ({entries_delta}) \
         — every admitted candidate contributes exactly one counter \
         increment under `publish_core`, with no double-count or under-count",
    );
    // The keyed canonical is registered in the reverse index (both
    // candidates self-root it).
    assert!(
        db.reverse_index_contains_for_test(&key),
        "the live key must be registered in the store's per-canonical \
         reverse index",
    );
    // A per-canonical invalidation drains BOTH candidates.
    db.invalidate_canonical(canonical);
    assert_eq!(
        db.live_count(),
        0,
        "`invalidate_canonical` must drain both coexisting candidates",
    );
}

// ===========================================================================
// Structural carriers — `MaterializeStructureDb` and `RefCycleResultDb`.
//
// These two query-identity caches carry an explicit `self_root_canonicals`
// set: `MaterializeStructureDb` roots the materialise SUBJECT's
// declaration-origin file — the extracted route root for a route-shaped
// subject, the `base` node's origin for a non-route subject (the consumer
// materialise scope is NEVER a self-root — R7 cross-owner reuse);
// `RefCycleResultDb` roots the BFS root
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
/// (it self-roots on the materialise SUBJECT's declaration-origin file —
/// the extracted route root for a route-shaped subject, the `base` node's
/// origin for a non-route subject — R7 cross-owner reuse), so to drive
/// the strict path deterministically a
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
    use crate::component_meta_materialize::{
        MaterializationCacheKey, MaterializationScope, MaterializeOutcome,
    };
    use crate::semantic_query::{ProjectionMode, ResolvedDeclSlotIdentity};

    let host = host_with_unrelated_file();
    let scope = "/struct_carrier_qdb/ms_never_loaded.ts";
    assert_untracked(&host, scope);
    let ctx: &dyn ResolverContext = &host;
    let db = host.project_type_store().materialize_structure_db();

    let base = intern_global_object(&host);
    // Content-free canonical-subject key. The self-root under test lives in
    // the planted carrier's facts + `self_root_canonicals` (the untracked
    // `scope`), NOT in the key, so any well-formed slot addresses the
    // planted candidate; seed the slot on `scope` for a stable identity.
    let key = MaterializationCacheKey {
        decl: ResolvedDeclSlotIdentity::type_slot_unscoped(Arc::from(scope), Arc::from("Probe")),
        projection_path: crate::resolver_core::RouteDemand::Whole,
        scope_axis: MaterializationScope::TopLevel,
        projection_mode: ProjectionMode::Expanded,
        normalized_type_args: Arc::from(Vec::new().into_boxed_slice()),
        resolve_env_hash: crate::semantic_query::HashValue::default(),
    };

    // Plant a synthetic candidate: the carrier's facts rail holds a
    // self-root `FileWholeHash` for the untracked scope, and
    // `self_root_canonicals` lists it. A lax validator admits this; the
    // strict one does not.
    let facts: Arc<[FactVersionRef]> = Arc::from(vec![FactVersionRef::FileWholeHash {
        canonical_id: scope.to_string(),
        hash: PLANTED_HASH,
    }]);
    db.insert_for_test(
        key.clone(),
        MaterializeOutcome::Value(base),
        crate::fact_signature_helpers::ReadSetSignature::new(facts),
        planted_self_root_canonicals(scope),
        // Live project generation — this test exercises the carrier's
        // strict self-root rejection, not the generation gate, so the
        // stamp must match the live generation.
        ctx.project_type_store().current_project_generation(),
    );

    assert!(
        db.peek(&key, ctx).is_none(),
        "MaterializeStructureDb::peek MUST reject a warm candidate whose self-root \
         FileWholeHash names an UNTRACKED scope canonical — the lax `validate` accepts \
         the untracked self-root and serves the candidate stale; only the strict \
         `validate_with_self_roots` rejects it.",
    );
}

/// Intern a DECL-ROOTED `base` — a `DeclRef` to `canonical:name` with a
/// `NodeScopeId::File` origin at `canonical`'s current whole hash. The
/// materialiser canonicalises this to `slot(canonical, name)` (so it
/// publishes a warm `MaterializeStructureDb` entry — an anonymous
/// `Object` base keys no slot), and its `base_origin_self_root` is
/// `canonical` (so a content edit to `canonical` rejects the entry, while
/// an edit to an unrelated consumer scope does not). The `base` node id —
/// hence the content-free cache key — is STABLE across an edit to
/// `canonical` (only the file's `whole_hash` the carrier records shifts).
fn intern_file_derived_decl_ref(
    host: &VerterHost,
    canonical: &str,
    name: &str,
) -> crate::semantic_query::SemanticNodeId {
    use crate::semantic_query::{DeclIdentity, NodeScopeId, SemanticNodeData};
    let whole_hash = host
        .shallow_file_state(canonical)
        .map(|s| s.whole_hash)
        .expect("decl-ref base fixture: canonical must be tracked with a whole hash");
    host.project_type_store()
        .semantic_graph()
        .intern_node_with_scope(
            SemanticNodeData::DeclRef {
                identity: DeclIdentity {
                    canonical_id: Arc::from(canonical),
                    whole_hash,
                    decl_name: Arc::from(name),
                },
            },
            NodeScopeId::File {
                canonical_id: Arc::from(canonical),
                whole_hash,
                local_scope: None,
            },
        )
}

/// Intern a ROUTE-SHAPED `base` — a builtin `Pick<Root, 'key'>`
/// `InstantiationRef` carrier interned at the `consumer_scope` file,
/// MIRRORING production lowering: `lower.rs` interns a member-value
/// `Pick<…>` carrier AND its inner `DeclRef{Root}` arg with
/// `scope.clone()` — i.e. the CONSUMER file scope (the reference site),
/// NOT `Root`'s declaration file. The carrier's extracted route root is
/// `root_decl_file:root_name`, so the materialiser canonicalises it to
/// `slot(root_decl_file, root_name)` + the `Pick('key')` route — and two
/// distinct consumer scopes over the same `Pick<Root,'key'>` collapse to
/// ONE entry (R7 cross-owner reuse). The materialised value is a pure
/// function of `Root`'s `key` member; it never depends on the consumer
/// wrapper file's content.
fn intern_consumer_scoped_pick_carrier(
    host: &VerterHost,
    consumer_scope: &str,
    root_decl_file: &str,
    root_name: &str,
    pick_key: &str,
) -> crate::semantic_query::SemanticNodeId {
    use crate::semantic_query::{DeclIdentity, NodeScopeId, SemanticNodeData};
    use verter_type_expr::LiteralValue;

    let consumer_hash = host
        .shallow_file_state(consumer_scope)
        .map(|s| s.whole_hash)
        .expect("route carrier fixture: consumer scope must be tracked with a whole hash");
    let root_hash = host
        .shallow_file_state(root_decl_file)
        .map(|s| s.whole_hash)
        .expect("route carrier fixture: root decl file must be tracked with a whole hash");
    let consumer_scope_id = NodeScopeId::File {
        canonical_id: Arc::from(consumer_scope),
        whole_hash: consumer_hash,
        local_scope: None,
    };
    let graph = host.project_type_store().semantic_graph();
    // Inner `DeclRef{Root}` — interned at the CONSUMER scope (the
    // reference site), exactly as production lowering interns an imported
    // name. Its `identity.canonical_id` points at `Root`'s declaration
    // file; its NODE scope is the consumer wrapper file.
    let root_ref = graph.intern_node_with_scope(
        SemanticNodeData::DeclRef {
            identity: DeclIdentity {
                canonical_id: Arc::from(root_decl_file),
                whole_hash: root_hash,
                decl_name: Arc::from(root_name),
            },
        },
        consumer_scope_id.clone(),
    );
    let key_literal = graph.intern_node_with_scope(
        SemanticNodeData::Literal(LiteralValue::String(pick_key.to_string())),
        consumer_scope_id.clone(),
    );
    // Builtin `Pick` carrier — interned at the CONSUMER scope. The
    // `__builtin__`/`Pick` identity is what `extract_route_root_identity_node`
    // matches; the whole_hash is irrelevant to that match.
    graph.intern_node_with_scope(
        SemanticNodeData::InstantiationRef {
            base: DeclIdentity {
                canonical_id: Arc::from("__builtin__"),
                whole_hash: [0u8; 16],
                decl_name: Arc::from("Pick"),
            },
            args: Arc::from(vec![root_ref, key_literal].into_boxed_slice()),
        },
        consumer_scope_id,
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
    use crate::component_meta_materialize::{
        derive_materialization_subject, MaterializationScope, MaterializeRuntimeKey,
    };
    use crate::semantic_query::ProjectionMode;

    let host = VerterHost::new_standalone(HostConfig::default());
    let edited_file = "/struct_carrier_qdb/ms_base_origin.ts";
    upsert(&host, edited_file, "export type Probe = { a: number };\n");
    assert!(
        host.ensure_indexed_ready(edited_file).is_some(),
        "edited_file IndexedReady materialises",
    );

    let dispatch = host.semantic_dispatch();
    // DECL-ROOTED `base` — a `DeclRef` to `edited_file:Probe` with origin
    // scope `NodeScopeId::File { edited_file }`. The materialiser
    // canonicalises it to `slot(edited_file, Probe)` (so the entry caches)
    // and self-roots it on `edited_file`.
    let base = intern_file_derived_decl_ref(&host, edited_file, "Probe");
    let runtime_key = MaterializeRuntimeKey {
        scope_canonical_id: Arc::from(edited_file),
        base,
        scope_axis: MaterializationScope::TopLevel,
        mode: ProjectionMode::Expanded,
    };
    // The content-free DB cache key, built by the SAME canonical builder
    // the materialiser uses. The slot is content-free, so this key is
    // STABLE across the content edit below.
    let key = derive_materialization_subject(&host, &runtime_key)
        .expect("a DeclRef base canonicalises to a MaterializationCacheKey subject");

    // Cold build — publishes a warm entry self-rooted on the `base`
    // node's declaration-origin file.
    let _ = dispatch.materialize_surface(runtime_key);
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
/// Discriminating property: the `base` is a DECL-ROOTED `DeclRef` to
/// `decl_file:Probe`, so its declaration-origin self-root is `decl_file`
/// (the slot's canonical, which the materialisation reads). The consumer
/// materialise scope is a SEPARATE file the materialisation never reads.
/// The cache key excludes `scope_canonical_id` (R7 cross-owner reuse), so
/// the cache key is stable across the scope edit.
///
/// - **Pre-fix tree:** `materialize_structure_read_set` seeds the
///   consumer scope into `self_root_hashes` as a strict self-root
///   `FileWholeHash`. A content edit to the scope shifts that
///   `FileWholeHash`, so strict `validate_with_self_roots` REJECTS the
///   entry — the warm `peek` misses.
/// - **Post-fix tree:** the consumer scope is NOT a self-root; the entry
///   self-roots on `decl_file` (the base node's declaration-origin file,
///   the non-route subject case). A content edit to the consumer scope —
///   which the materialisation never read — leaves the entry's signature
///   untouched, so the warm `peek` still HITS.
///
/// This test FAILS against the pre-fix tree (the artificial scope
/// self-root invalidates the warm entry) and PASSES post-fix. It is the
/// direct discriminator for the consumer-scope over-rooting removal.
#[test]
fn materialize_structure_db_unread_scope_edit_keeps_warm_entry() {
    use crate::component_meta_materialize::{
        derive_materialization_subject, MaterializationScope, MaterializeRuntimeKey,
    };
    use crate::semantic_query::ProjectionMode;

    let host = VerterHost::new_standalone(HostConfig::default());
    // The DECL file the `base` roots in, and a SEPARATE consumer scope the
    // materialisation never reads.
    let decl_file = "/struct_carrier_qdb/ms_unread_decl.ts";
    let scope = "/struct_carrier_qdb/ms_unread_scope.ts";
    upsert(&host, decl_file, "export type Probe = { a: number };\n");
    upsert(&host, scope, "export const anchor = 1;\n");
    assert!(
        host.ensure_indexed_ready(decl_file).is_some()
            && host.ensure_indexed_ready(scope).is_some(),
        "decl_file + scope IndexedReady materialise",
    );

    let dispatch = host.semantic_dispatch();
    // DECL-ROOTED `base` — a `DeclRef` to `decl_file:Probe`. Its
    // declaration-origin self-root is `decl_file`, NOT the consumer
    // `scope`: the materialisation reads `decl_file` (the slot's
    // canonical), never the consumer scope. So an edit to `scope` must
    // leave the entry's signature untouched.
    let base = intern_file_derived_decl_ref(&host, decl_file, "Probe");
    let runtime_key = MaterializeRuntimeKey {
        scope_canonical_id: Arc::from(scope),
        base,
        scope_axis: MaterializationScope::TopLevel,
        mode: ProjectionMode::Expanded,
    };
    let key = derive_materialization_subject(&host, &runtime_key)
        .expect("a DeclRef base canonicalises to a MaterializationCacheKey subject");

    // Cold build — publishes a warm entry self-rooted on `decl_file`, NOT
    // the consumer `scope`.
    let _ = dispatch.materialize_surface(runtime_key);
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

/// **Route-shaped cross-owner reuse — value self-roots at the EXTRACTED
/// ROOT, never the first producer's consumer wrapper file.**
///
/// Binds the cx-revb blocker. A route-shaped subject (`Pick`/`Omit`/
/// IndexedAccess) is keyed on the EXTRACTED ROOT slot + route path, so
/// owners A and B both consuming `Pick<Shared,'id'>` collapse to ONE
/// `MaterializeStructureDb` entry. The materialised value is a pure
/// function of `Shared`'s `id` member — the route compute reads
/// `Shared` (via `Instantiate`), NEVER either wrapper file. So the
/// entry MUST self-root on `Shared`'s declaration file, NOT on the
/// FIRST PRODUCER's (A's) consumer wrapper scope (where the `Pick`
/// carrier was interned).
///
/// Two directions, both required:
///
/// - **Direction 1 (the false-miss blocker, RED pre-fix):** edit ONLY
///   owner A's wrapper file (Shared + owner B untouched). Owner B's
///   warm reuse MUST survive. Pre-fix the entry's
///   `base_origin_self_root` is the wrapper carrier's CONSUMER scope
///   (owner A) — so A's shifted `FileWholeHash` rejects B's read, a
///   migration-induced R7 cross-owner false miss. Post-fix the
///   self-root is `Shared`, so A's edit leaves the entry untouched and
///   B HITs.
/// - **Direction 2 (no stale reuse):** edit `Shared`'s `id` member. The
///   value DOES depend on it, so the warm reuse MUST be invalidated on
///   the extracted-root self-root.
///
/// The cache key is content-free (R6), so it is STABLE across both
/// edits — invalidation rides on the value-side self-root, never the
/// key.
#[test]
fn route_shaped_materialize_self_roots_at_extracted_root_not_consumer_wrapper() {
    use crate::component_meta_materialize::{
        derive_materialization_subject, MaterializationScope, MaterializeRuntimeKey,
    };
    use crate::semantic_query::ProjectionMode;

    let host = VerterHost::new_standalone(HostConfig::default());
    let shared = "/route_self_root/shared.ts";
    let owner_a = "/route_self_root/A.ts";
    let owner_b = "/route_self_root/B.ts";
    upsert(
        &host,
        shared,
        "export interface Shared { id: string; body: string; }\n",
    );
    upsert(&host, owner_a, "export const a = 1;\n");
    upsert(&host, owner_b, "export const b = 1;\n");
    for f in [shared, owner_a, owner_b] {
        assert!(
            host.ensure_indexed_ready(f).is_some(),
            "{f} IndexedReady materialises",
        );
    }

    let dispatch = host.semantic_dispatch();
    // Two ROUTE-SHAPED `Pick<Shared,'id'>` carriers, one interned at each
    // owner's CONSUMER scope.
    let carrier_a = intern_consumer_scoped_pick_carrier(&host, owner_a, shared, "Shared", "id");
    let carrier_b = intern_consumer_scoped_pick_carrier(&host, owner_b, shared, "Shared", "id");

    let runtime_a = MaterializeRuntimeKey {
        scope_canonical_id: Arc::from(owner_a),
        base: carrier_a,
        scope_axis: MaterializationScope::TopLevel,
        mode: ProjectionMode::Expanded,
    };
    let runtime_b = MaterializeRuntimeKey {
        scope_canonical_id: Arc::from(owner_b),
        base: carrier_b,
        scope_axis: MaterializationScope::TopLevel,
        mode: ProjectionMode::Expanded,
    };
    // The content-free DB key both owners canonicalise to — the extracted
    // root (Shared) slot + the Pick route, with NO consumer-scope
    // dimension. Identical for A and B; STABLE across the edits below.
    let key_a = derive_materialization_subject(&host, &runtime_a)
        .expect("route-shaped carrier canonicalises to a MaterializationCacheKey subject");
    let key_b = derive_materialization_subject(&host, &runtime_b)
        .expect("route-shaped carrier canonicalises to a MaterializationCacheKey subject");
    assert_eq!(
        key_a, key_b,
        "fixture invariant: both owners' Pick<Shared,'id'> carriers MUST canonicalise to \
         ONE shared content-free subject key (R7 cross-owner reuse) — the extracted root \
         (Shared) slot + Pick route, with NO consumer-scope dimension",
    );

    // Owner A cold-builds the shared entry.
    let _ = dispatch.materialize_surface(runtime_a);
    let db = host.project_type_store().materialize_structure_db();
    assert!(
        db.peek(&key_b, &host).is_some(),
        "fixture invariant: owner A's cold build admitted a warm entry that owner B \
         (the SAME shared subject) reuses while all files are unedited",
    );

    // ── Direction 1: edit ONLY owner A's wrapper file. Shared + owner B
    // are untouched.
    upsert(
        &host,
        owner_a,
        "export const a = 999;\nexport const extra = 2;\n",
    );
    assert!(
        host.ensure_indexed_ready(owner_a).is_some(),
        "owner A IndexedReady re-materialises after the edit",
    );
    assert!(
        db.peek(&key_b, &host).is_some(),
        "ROUTE-SHAPED CROSS-OWNER FALSE-MISS: editing owner A's wrapper file MUST NOT \
         invalidate owner B's warm reuse of the shared `Pick<Shared,'id'>` entry. The \
         entry's value is a pure function of Shared's `id` member and never read A's \
         wrapper text; rooting it on the FIRST PRODUCER's (A's) consumer wrapper scope \
         over-roots the shared entry and defeats R7 cross-owner reuse. Pre-fix \
         `base_origin_self_root` = the wrapper carrier's consumer scope (owner A), so A's \
         shifted FileWholeHash strictly rejects B's read.",
    );

    // ── Direction 2: edit Shared's `id` member — the value DOES depend on
    // it, so the warm reuse MUST be invalidated.
    upsert(
        &host,
        shared,
        "export interface Shared { id: number; body: string; }\n",
    );
    assert!(
        host.ensure_indexed_ready(shared).is_some(),
        "Shared IndexedReady re-materialises after the edit",
    );
    assert!(
        db.peek(&key_b, &host).is_none(),
        "STALE REUSE: editing Shared's `id` member MUST invalidate the shared \
         `Pick<Shared,'id'>` entry — the materialised value is a function of Shared's `id` \
         member, so a content edit to Shared's declaration file rejects the warm entry on \
         its extracted-root self-root.",
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

    // Plant a synthetic candidate: the carrier's facts rail holds a
    // self-root `FileWholeHash` for the untracked root and
    // `self_root_canonicals` lists the root. A lax validator admits this;
    // the strict one does not.
    let facts: Arc<[FactVersionRef]> = Arc::from(vec![FactVersionRef::FileWholeHash {
        canonical_id: root.to_string(),
        hash: PLANTED_HASH,
    }]);
    db.insert_for_test(
        &id,
        ctx,
        true,
        crate::fact_signature_helpers::ReadSetSignature::new(facts),
        planted_self_root_canonicals(root),
        // Live project generation — this test exercises the carrier's
        // strict self-root rejection, not the generation gate, so the
        // stamp must match the live generation.
        ctx.project_type_store().current_project_generation(),
    );

    assert!(
        crate::component_meta_caches::ref_cycle_db_peek(db, &id, ctx).is_none(),
        "RefCycleResultDb::peek MUST reject a warm candidate whose self-root FileWholeHash \
         names an UNTRACKED root canonical — the lax `validate` accepts the untracked \
         self-root and serves the candidate stale; only the strict `validate_with_self_roots` \
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
    // Universal `ShapeCacheDb` — replaces the previously-
    // split `materialize_memo_db` (TypeExpr subject) +
    // `member_shape_cache_db` (MemberValueNode subject). Both subjects
    // share the same cache substrate; each retains its own
    // self-root discriminator test under the unified DB name.
    (
        "shape_cache_db",
        "materialize_memo_db_untracked_self_root_rejects_warm_entry",
    ),
    (
        "shape_cache_db",
        "member_value_node_cache_db_untracked_self_root_rejects_warm_entry",
    ),
    (
        "materialize_structure_db",
        "materialize_structure_db_planted_untracked_self_root_rejects_warm_entry",
    ),
    (
        "ref_cycle_db",
        "ref_cycle_db_untracked_self_root_rejects_warm_entry",
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
        "compile_cache_db",
        "derived_raw_cache_db",
        "dependency_cache_db",
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
    let primed = db.get_or_compute_admit_traced_for_test(&key, ctx, || {
        let validated_at_generation = host.project_type_store().current_project_generation();
        crate::cache_runtime::singleflight::ComputeAdmission::Cacheable(
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
/// project-generation mismatch skips the stale candidate on read (it
/// stays resident); when that candidate is later removed through the
/// per-canonical drain the cache's COMPLETE removal cleanup runs — the
/// substrate's `removal_cleanup` closure unregisters the drained
/// candidate's `CanonicalIndex` reverse-index registration.
///
/// `ImportedRegistryDb` is the only fence-less cache that carries a
/// reverse index, so it is the one Part-C coupling concern: adding the
/// generation gate to the cooperative `validate` closure creates a new
/// reject path, and that reject must clean the reverse index, not just
/// skip it on read. The store keeps a stale candidate (it may still be
/// valid for another view); routine reclamation is the per-canonical
/// drain + FIFO budget, NOT a reap-on-read.
///
/// Discriminating property — a candidate is primed through the production
/// cold path (stamps `validated_at_generation`, registers the reverse
/// index). A bare `bump_project_generation()` advances the counter. The
/// next `get_or_compute_admit` reaches its warm-hit `lookup_candidate`:
///
/// - With the generation gate in `lookup_candidate`, the stale candidate
///   is SKIPPED, the cooperative cold path runs (`cold_ran == true`), and
///   the candidate stays resident (no reap-on-read). Per-canonical
///   invalidation then reclaims it AND drops its reverse-index
///   registration.
/// - Without the generation gate, the warm read would accept the stale
///   candidate by its file-content-only signature and short-circuit before
///   `compute` (`cold_ran == false`), failing the discriminator below.
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
    let primed = db.get_or_compute_admit_traced_for_test(&key, ctx, || {
        let validated_at_generation = host.project_type_store().current_project_generation();
        crate::cache_runtime::singleflight::ComputeAdmission::Cacheable(
            crate::component_meta_caches::ImportedRegistryEntry {
                value: None,
                fact_dep_signature: empty_fact_signature(),
                validated_at_generation,
            },
        )
    });
    assert!(
        primed.is_some(),
        "fixture invariant: the cold cooperative admission publishes the candidate",
    );
    assert!(
        db.reverse_index_contains_for_test(&key),
        "fixture invariant: the primed candidate registered `key` in the \
         per-canonical reverse index",
    );

    // Advance the project generation WITHOUT evicting any cache.
    let g_after_bump = host.project_type_store().bump_project_generation();
    assert!(
        g_after_bump > 0,
        "fixture invariant: the project generation advanced",
    );

    // A second cooperative admission: its warm-hit `lookup_candidate`
    // re-reads the stale candidate. The generation gate rejects it, so the
    // candidate is SKIPPED and the cooperative cold path runs. The
    // `compute` closure returns `ReturnOnly` so nothing republishes; the
    // refusal reason is `GenerationSuperseded` because the cold compute
    // declined to admit on generation grounds (the project generation
    // advanced between the prime and the second admission).
    let mut cold_ran = false;
    let _ = db.get_or_compute_admit_traced_for_test(&key, ctx, || {
        cold_ran = true;
        let _reason_guard = crate::cache_runtime::SetReasonGuard::arm(
            crate::cache_runtime::NonAdmissionReason::GenerationSuperseded,
        );
        crate::cache_runtime::singleflight::ComputeAdmission::ReturnOnly(None)
    });
    assert!(
        cold_ran,
        "DISCRIMINATOR: the warm-hit `lookup_candidate` MUST reject the \
         generation-stale candidate so the cooperative cold path runs — a \
         lookup that accepted the stale candidate on its file-content-only \
         signature would short-circuit before `compute`. The generation \
         gate rides inside `lookup_candidate`.",
    );

    // The store does NOT reap a stale candidate on read — it stays for
    // other views and for budget / per-canonical reclamation.
    assert!(
        db.reverse_index_contains_for_test(&key),
        "the store keeps the generation-stale candidate (and its \
         reverse-index registration) resident on read — reclamation is the \
         per-canonical drain + FIFO budget, not a reap-on-read",
    );

    // Per-canonical invalidation reclaims the stale candidate AND drops
    // its reverse-index registration in one O(K) drain.
    db.invalidate_canonical(canonical);
    assert!(
        !db.reverse_index_contains_for_test(&key),
        "`invalidate_canonical` must drop the drained candidate's \
         reverse-index registration",
    );
    assert_eq!(
        db.live_count(),
        0,
        "`invalidate_canonical` must drain the generation-stale candidate \
         (the `ReturnOnly` cold outcome never republished)",
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
    let outcome = db.get_or_compute_traced_for_test(&key, ctx, || {
        host.project_type_store().bump_project_generation();
        ComputedEntry::Rooted(decl("stale", c), empty_fact_signature())
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

    let outcome = db.get_or_compute_traced_for_test(&key, ctx, || {
        host.project_type_store().bump_project_generation();
        ComputedEntry::Rooted(true, empty_fact_signature())
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

    let outcome = db.get_or_compute_traced_for_test(&key, ctx, || {
        host.project_type_store().bump_project_generation();
        ComputedEntry::Rooted(None, empty_fact_signature())
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

/// `MaterializeMemoDb` — failed-revalidation cold compute must not leak the
/// shared live counter. See
/// [`declaration_lookup_failed_revalidation_does_not_leak_live_counter`].
#[test]
fn materialize_memo_failed_revalidation_does_not_leak_live_counter() {
    use crate::semantic_query::SemanticNodeId;
    let host = host_with_unrelated_file();
    let c = "/live_counter_qdb/materialize_memo.ts";
    let ctx: &dyn ResolverContext = &host;
    let db = host.project_type_store().shape_cache_db();
    let key = crate::component_meta_caches::ShapeCacheKey::member_value_node_whole_for_test(
        Arc::<str>::from(c),
        SemanticNodeId(7301),
        ProjectionMode::Shallow,
    );

    let counter_before = component_meta_cache_live(&host);
    let map_before = db.live_count();

    let outcome = db.get_or_compute_traced_for_test(&key, ctx, || {
        host.project_type_store().bump_project_generation();
        Some((
            MaterializedOutputTypeExpr::from_type_expr_for_test(
                None,
                TypeExpr::Unknown { raw: String::new() },
                Arc::from([] as [(Arc<str>, crate::semantic_query::DepVersion); 0]),
                false,
            ),
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
    use crate::semantic_query::SemanticNodeId;

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
            + store.shape_cache_db().live_count()
            + store.materialize_structure_db().live_count()
            + store.ref_cycle_db().live_count()
    };

    // Drive a failed-revalidation cold compute through each of the
    // `cooperative_get_or_insert` engine DBs.
    let bump = || {
        host.project_type_store().bump_project_generation();
    };

    let _ = store.declaration_db().get_or_compute_traced_for_test(
        &(Arc::<str>::from("/lc_total/decl.ts"), Arc::<str>::from("P")),
        ctx,
        || {
            bump();
            ComputedEntry::Rooted(decl("stale", "/lc_total/decl.ts"), empty_fact_signature())
        },
    );
    let _ = store.resolvable_db().get_or_compute_traced_for_test(
        &(
            Arc::<str>::from("/lc_total/resolv.ts"),
            Arc::<str>::from("P"),
        ),
        ctx,
        || {
            bump();
            ComputedEntry::Rooted(true, empty_fact_signature())
        },
    );
    let _ = store.owner_collection_db().get_or_compute_traced_for_test(
        &(
            Arc::<str>::from("/lc_total/owner.ts"),
            Arc::<str>::from("P"),
        ),
        ctx,
        || {
            bump();
            ComputedEntry::Rooted(None, empty_fact_signature())
        },
    );
    let _ = store.shape_cache_db().get_or_compute_traced_for_test(
        &crate::component_meta_caches::ShapeCacheKey::member_value_node_whole_for_test(
            Arc::<str>::from("/lc_total/memo.ts"),
            SemanticNodeId(7302),
            ProjectionMode::Shallow,
        ),
        ctx,
        || {
            bump();
            Some((
                MaterializedOutputTypeExpr::from_type_expr_for_test(
                    None,
                    TypeExpr::Unknown { raw: String::new() },
                    Arc::from([] as [(Arc<str>, crate::semantic_query::DepVersion); 0]),
                    false,
                ),
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
    // Synthetic insert+lookup with a hand-built key: the env axes only
    // need to be CONSISTENT between the planted entry and the lookup
    // (this test exercises the project-generation gate, not env
    // discrimination), so uniform zeros are correct here.
    let key = ComponentMetaResultKey {
        owner_canonical: Arc::from(owner),
        options_fingerprint: [0u8; 16],
        project_identity: crate::file_artifact_store::ProjectIdentity([0u8; 16]),
        parse_env_hash: [0u8; 16],
        resolve_env_hash: [0u8; 16],
        type_env_hash: [0u8; 16],
        lib_env_hash: [0u8; 16],
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
            completeness: crate::semantic_query::ResultCompleteness::Complete,
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

    let view = host
        .resolver_store_view_read()
        .current()
        .expect("a quiescent host must yield a current store view");
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

    let view_after = host
        .resolver_store_view_read()
        .current()
        .expect("a quiescent host must yield a current store view");
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
    use crate::semantic_query::{
        PrimitiveKind, RelateMemoKey, RelationContext, RelationResult, SemanticNodeData,
        SemanticNodeId,
    };
    use crate::semantic_query_memo::SemanticGraphStore;

    let host = VerterHost::new_standalone(HostConfig::default());
    let ctx: &dyn ResolverContext = &host;
    let store = SemanticGraphStore::new();
    let source: SemanticNodeId =
        store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let target: SemanticNodeId =
        store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let key = RelateMemoKey::assignable(source, target, RelationContext::default());

    // Plant a relation judgement with a valid (empty + empty
    // self-roots) carrier tagged at the CURRENT project generation.
    let gen0 = host.project_type_store().current_project_generation();
    store.insert_relation(
        key.clone(),
        crate::fact_signature_helpers::ReadSetSignature::empty(),
        Arc::from(Vec::<Arc<str>>::new()),
        RelationResult::NotAssignable,
        gen0,
    );

    // Same generation — the carrier validates vacuously and the
    // generation matches, so `get_relation` HITs.
    assert!(
        store.get_relation(ctx, &key).is_some(),
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
        store.get_relation(ctx, &key).is_none(),
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

    let view = host.resolver_store_view_read().into_owned_view();

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

    let view_after = host.resolver_store_view_read().into_owned_view();
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

// ===========================================================================
// `ShapeSubject::MemberValueNode` — the member-value equivalence class and
// cross-view fail-closed contract.
//
// The sealed member-shape subject keys on `scope + MemberShapeNodeSubject`
// (the member's `SurfaceMember.value` graph node) + `demand` — NOT on the
// member's name or any other metadata. Two consequences the two tests
// below pin behaviorally (not by reading the module-private `subject`):
//
//   (8a) EQUIVALENCE PRESERVATION — sibling members whose `SurfaceMember.value`
//        is the same settled graph node collapse onto ONE warm entry. This is
//        the carve-out's whole point: a per-member route must dedup across
//        siblings that share a value node, while a DIFFERENT value node keeps
//        a disjoint entry.
//   (8b) CROSS-VIEW FAIL-CLOSED — a member-value shape computed under a
//        session overlay is NOT served to the base view (and vice versa). The
//        single-entry slot may be displaced across incompatible views, but it
//        must NEVER stale-serve: the strict `ReadSetSignature` self-root
//        validation roots the entry on the view-authoritative content hash, so
//        a read from the other view recomputes cold.
// ===========================================================================

/// Build a [`crate::semantic_query::SurfaceMember`] with an explicit name and
/// value node. Mirrors the production `required_member` shape: a `Public`,
/// non-optional, non-method, own-body authored member.
fn shape_member(
    name: &str,
    value: crate::semantic_query::SemanticNodeId,
) -> crate::semantic_query::SurfaceMember {
    crate::semantic_query::SurfaceMember {
        visibility: verter_type_expr::MemberVisibility::Public,
        name: Arc::from(name),
        value,
        optional: false,
        readonly: false,
        is_method: false,
        declared_in_macro_type_arg: crate::semantic_query::MacroOwnBodyStamp::NEUTRAL,
        merge_role: crate::semantic_query::MergeRoleStamp::NEUTRAL,
        spans: Default::default(),
        declaration_origin: None,
    }
}

/// A self-root `FileWholeHash` signature for `canonical` pinned to the
/// supplied `hash` — the exact content version the cache entry is rooted on.
/// The strict warm-read validator checks this against the live view's
/// authoritative self-root hash for the keyed canonical.
fn self_root_at(canonical: &str, hash: [u8; 16]) -> Arc<[FactVersionRef]> {
    Arc::from(vec![FactVersionRef::FileWholeHash {
        canonical_id: canonical.to_string(),
        hash,
    }])
}

/// (8a) Equivalence preservation. Two DISTINCT `SurfaceMember`s (different
/// `name`/`optional`) that share the SAME `.value` graph node must produce
/// the SAME `ShapeSubject::MemberValueNode` cache slot — the second
/// `get_or_compute` is a WARM HIT of the first, so its cold closure does NOT
/// run and `live_count()` stays 1. A member with a DIFFERENT `.value` node
/// must NOT collapse — it keys a disjoint entry, runs its own cold closure,
/// and grows `live_count()` to 2.
///
/// Two layers of discrimination:
///
///  - **Constructor-level** (`ShapeCacheKey::surface_member_value_whole_with_context`):
///    the key built from two same-`.value` members is byte-equal, and the
///    key built from a different-`.value` member is distinct. This is the
///    exact equivalence the production cache keys ON.
///  - **Production-seam-level** (`surface_member_to_expanded_field` ->
///    `member_shape_peek_or_compute`): the REAL per-member peek/compute path
///    is driven with real `SurfaceMember`s. The sibling sharing `.value`
///    warm-hits the entry the first member admitted (so `live_count()` stays
///    1 across the two `surface_member_to_expanded_field` calls), and a
///    different-`.value` member admits a disjoint entry (`live_count()`
///    grows to 2). This routes through the SAME `surface_member_value_whole_with_context`
///    key the production projector uses, so it discriminates the production
///    behaviour, not only the constructor.
///
/// Discriminating property: the assertion that the same-`.value` sibling does
/// NOT grow the cache trips RED if the cache subject keyed on the member NAME
/// (or any per-member metadata) instead of `member.value` — two
/// differently-named members would then split into two entries. The
/// complementary different-`.value` assertion trips RED if the subject
/// collapsed every member onto one slot regardless of value node.
#[test]
fn member_value_node_equivalence_class_collapses_siblings_sharing_value_node() {
    use crate::component_meta_caches::ShapeCacheKey;
    use crate::semantic_query::{ProjectionMode, ProjectionReductionContext, SemanticNodeId};

    let host = VerterHost::new_standalone(HostConfig::default());
    let c = "/member_value_eq/scope.ts";
    // A real, tracked scope so the entry's self-root validates and the
    // warm path is ADMITTED (the *_untracked_* rail tests force the
    // opposite — rejection; here we need a genuine warm hit to prove the
    // equivalence-class collapse).
    upsert(&host, c, "export type Probe = number;\n");
    host.ensure_indexed_ready(c)
        .expect("scope IndexedReady materialises");
    let ctx: &dyn ResolverContext = &host;
    let scope_hash = observed_whole_hash(ctx, c);
    let db = host.project_type_store().shape_cache_db();

    // The shared value node both siblings reference.
    let shared_value = SemanticNodeId(41);
    let mode = ProjectionMode::Expanded;
    let context = ProjectionReductionContext::published(mode);

    // Two DISTINCT members (different name + optionality) over the SAME
    // `.value`. Keys built through the `#[cfg(test)]` raw member ctor (the
    // production path keys from an admitted publication token; this probe
    // asserts the subject identity, not admission).
    let member_a = shape_member("alpha", shared_value);
    let mut member_b = shape_member("beta", shared_value);
    member_b.optional = true;
    let key_a = ShapeCacheKey::surface_member_value_whole_with_context_raw(
        Arc::<str>::from(c),
        &member_a,
        context,
    );
    let key_b = ShapeCacheKey::surface_member_value_whole_with_context_raw(
        Arc::<str>::from(c),
        &member_b,
        context,
    );
    assert_eq!(
        key_a, key_b,
        "two distinct SurfaceMembers sharing the same `.value` node MUST build the \
         SAME member-value cache key — the subject keys on `scope + member.value + \
         demand`, never on the member name or optionality",
    );

    // Prime the slot through member A. A self-root at the scope's real
    // current hash validates, so the entry is ADMITTED.
    let primed = db
        .get_or_compute_traced_for_test(&key_a, ctx, || {
            Some((
                materialized("alpha-shape", empty_dep_signature()),
                self_root_at(c, scope_hash),
            ))
        })
        .expect("member A primes a warm entry");
    assert!(
        matches!(&primed.type_expr_for_test(), TypeExpr::Unknown { raw } if raw == "alpha-shape"),
        "fixture invariant: the primed entry carries member A's shape",
    );
    assert_eq!(
        db.live_count(),
        1,
        "the primed member-value entry must be admitted (live_count == 1)",
    );

    // Sibling member B (same `.value`, different name) MUST warm-hit the
    // entry primed by member A — its cold closure must NOT run.
    let mut sibling_cold_ran = false;
    let sibling = db
        .get_or_compute_traced_for_test(&key_b, ctx, || {
            sibling_cold_ran = true;
            Some((
                materialized("beta-shape", empty_dep_signature()),
                self_root_at(c, scope_hash),
            ))
        })
        .expect("sibling member B resolves");
    assert!(
        !sibling_cold_ran,
        "EQUIVALENCE-CLASS BROKEN: a sibling member sharing the same `SurfaceMember.value` \
         node ran its OWN cold closure — the per-member subject must collapse siblings \
         that share a value node onto ONE warm entry, not split per member name/metadata",
    );
    assert!(
        matches!(&sibling.type_expr_for_test(), TypeExpr::Unknown { raw } if raw == "alpha-shape"),
        "the sibling must receive member A's WARM shape (`alpha-shape`), not recompute \
         its own — proving the collapse onto one entry",
    );
    assert_eq!(
        db.live_count(),
        1,
        "the sibling warm hit must NOT grow the cache — `live_count()` stays 1",
    );

    // NEGATIVE / discriminating: a member with a DIFFERENT `.value` node
    // must NOT collapse — it keys a disjoint entry and runs its cold closure.
    let member_other = shape_member("alpha", SemanticNodeId(99));
    let key_other = ShapeCacheKey::surface_member_value_whole_with_context_raw(
        Arc::<str>::from(c),
        &member_other,
        context,
    );
    assert_ne!(
        key_a, key_other,
        "a member with a DIFFERENT `.value` node MUST build a DISTINCT key — the value \
         node is the subject identity",
    );
    let mut other_cold_ran = false;
    let _ = db
        .get_or_compute_traced_for_test(&key_other, ctx, || {
            other_cold_ran = true;
            Some((
                materialized("other-shape", empty_dep_signature()),
                self_root_at(c, scope_hash),
            ))
        })
        .expect("the different-value member resolves");
    assert!(
        other_cold_ran,
        "a member with a DIFFERENT `.value` node must NOT warm-hit the shared entry — \
         its cold closure MUST run (the subject does not collapse unrelated value nodes)",
    );
    assert_eq!(
        db.live_count(),
        2,
        "the different-value member must add a SECOND entry (live_count == 2)",
    );

    // ---- Production-seam exercise ----
    // Drive the REAL per-member peek/compute path through the production
    // helper `surface_member_to_expanded_field` (which calls
    // `member_shape_peek_or_compute` in the `output_sink` sink module) so the
    // sibling-collapse is asserted through the production seam, not only the
    // directly-built key. A fresh scope keeps the seam's own admitted entries
    // disjoint from the directly-keyed entries above.
    use crate::meta_resolve::projection_demand::{PublishedSurfaceKind, SurfaceProjection};
    use crate::meta_resolve::projectors::output_sink::surface_member_to_expanded_field;
    use crate::meta_resolve::projectors::publication_authority::AdmittedPublishedMember;
    use crate::resolver_core::ComponentMetaQueryEngine;
    use crate::semantic_query::DeclIdentity;

    let seam_scope = "/member_value_eq/seam_scope.ts";
    upsert(&host, seam_scope, "export type SeamProbe = number;\n");
    host.ensure_indexed_ready(seam_scope)
        .expect("seam scope IndexedReady materialises");
    let seam_db = host.project_type_store().shape_cache_db();
    let seam_baseline = seam_db.live_count();

    // A whole-surface `Props` cursor publishes at `Expanded` (matches the
    // constructor-level `mode` above), so the seam's per-member reduction
    // context aligns with the keys exercised above. The production seam now
    // consumes a policy-admitted token; the cache-rail probe builds the token
    // directly via the `#[cfg(test)]` test ctor with synthetic member + cursor
    // values (it asserts the per-member cache subject, not admission policy).
    let projection = SurfaceProjection::whole_surface(PublishedSurfaceKind::Props);
    let seam_shared = SemanticNodeId(73);
    let seam_member_a = shape_member("seamAlpha", seam_shared);
    let mut seam_member_b = shape_member("seamBeta", seam_shared);
    seam_member_b.optional = true;
    let seam_member_other = shape_member("seamAlpha", SemanticNodeId(88));
    let seam_owner = DeclIdentity {
        canonical_id: std::sync::Arc::from(seam_scope),
        whole_hash: Default::default(),
        decl_name: std::sync::Arc::from("<sfc-script-setup>"),
    };

    let mut engine = ComponentMetaQueryEngine::new(&host);

    // First member admits one entry through the production seam.
    let admitted_a = AdmittedPublishedMember::admitted_for_test(
        seam_owner.clone(),
        seam_shared,
        seam_member_a,
        projection.cursor(),
        PublishedSurfaceKind::Props,
    );
    let _field_a = surface_member_to_expanded_field(
        &mut engine,
        seam_scope,
        &admitted_a,
        None,
        None,
        None,
        crate::meta_resolve::projectors::output_sink::MemberValuePosition::ShallowMember,
    );
    let after_seam_a = seam_db.live_count();
    assert_eq!(
        after_seam_a,
        seam_baseline + 1,
        "PRODUCTION SEAM: the first member must admit exactly one member-value \
         entry through `surface_member_to_expanded_field`",
    );

    // The sibling sharing `.value` MUST collapse onto member A's entry — the
    // production peek hits it, so the cache does NOT grow.
    let admitted_b = AdmittedPublishedMember::admitted_for_test(
        seam_owner.clone(),
        seam_shared,
        seam_member_b,
        projection.cursor(),
        PublishedSurfaceKind::Props,
    );
    let _field_b = surface_member_to_expanded_field(
        &mut engine,
        seam_scope,
        &admitted_b,
        None,
        None,
        None,
        crate::meta_resolve::projectors::output_sink::MemberValuePosition::ShallowMember,
    );
    assert_eq!(
        seam_db.live_count(),
        seam_baseline + 1,
        "PRODUCTION SEAM: a sibling member sharing `SurfaceMember.value` must \
         WARM-HIT the entry the first member admitted through \
         `member_shape_peek_or_compute` — the per-member subject collapses \
         siblings onto ONE entry, so `live_count()` must NOT grow",
    );

    // A different-`.value` member admits a disjoint entry through the seam.
    let admitted_other = AdmittedPublishedMember::admitted_for_test(
        seam_owner,
        SemanticNodeId(88),
        seam_member_other,
        projection.cursor(),
        PublishedSurfaceKind::Props,
    );
    let _field_other = surface_member_to_expanded_field(
        &mut engine,
        seam_scope,
        &admitted_other,
        None,
        None,
        None,
        crate::meta_resolve::projectors::output_sink::MemberValuePosition::ShallowMember,
    );
    assert_eq!(
        seam_db.live_count(),
        seam_baseline + 2,
        "PRODUCTION SEAM: a member with a DIFFERENT `.value` node must admit a \
         SECOND, disjoint entry through `surface_member_to_expanded_field` — \
         the subject does not collapse unrelated value nodes",
    );
}

/// ITEM 3 (residual same-class close) — the SURFACE-member publication path's
/// COLD-REDUCE admission (`surface_member_to_expanded_field` ->
/// `member_shape_peek_or_compute` in the `output_sink` sink) must REFUSE
/// `ShapeCacheDb` admission when the graph-native reduce consumed a FENCED
/// (ReturnOnly, `store_published == false`) `IndexedReady` serve — the twin of the
/// registry-member stabiliser hole
/// (`fenced_serve_shape_cache_member_value_is_not_admitted`).
///
/// A fenced serve is non-cacheable but NOT partial, so the
/// `MaterializedOutputTypeExpr` `result_is_partial()`-only admission gate cannot
/// reject it; the nested fact tracer wrapping the cold reduce (the `RefCycleResultDb`
/// / `app_config_no_override_proof` / `ResolvabilityDb` sibling pattern, identical to
/// the stabiliser twin) is the only rail that does. The subject here is a `typeof`
/// value node, which is a reducible operator (`needs_reduction == true`), so it
/// routes through the cold-reduce admit this close protects.
///
/// DISCRIMINATING: `force_indexed_ready_serve_fence_for_tests` fences every
/// `ensure_indexed_ready_serve` the reduce drives at a STABLE generation (no bump —
/// so a `GenerationSuperseded` gate cannot mask the refusal, and the served
/// `indexed` still reduces the shape). The unfenced control admits the shape
/// (`live_count` grows); the fenced request must NOT (`live_count` unchanged) while
/// the request stays `Complete` (the fenced serve routes through the fact tracer,
/// never the request partial sticky). RED-pre (drop the cold-reduce
/// `reduce_non_cacheable` refusal) the fenced shape LANDS in `ShapeCacheDb`.
#[test]
fn fenced_serve_surface_member_shape_is_not_admitted() {
    use crate::meta_resolve::projection_demand::{PublishedSurfaceKind, SurfaceProjection};
    use crate::meta_resolve::projectors::output_sink::{
        surface_member_to_expanded_field, MemberValuePosition,
    };
    use crate::meta_resolve::projectors::publication_authority::AdmittedPublishedMember;
    use crate::request_context::{RequestContext, RequestContextGuard};
    use crate::resolver_core::ComponentMetaQueryEngine;
    use crate::semantic_query::{DeclIdentity, ScopeId, SemanticNodeData, ValueRootKey};
    use std::sync::atomic::Ordering;

    // Drive the REAL surface-member seam for a `typeof <missing>` member value
    // interned in `scope`'s graph: its reduce drives an `ensure_indexed_ready_serve`
    // (so the fence has a serve to catch) and settles a deferred carrier that IS
    // admitted warm (so the control is a genuine admission). Returns the post-drive
    // ShapeCacheDb `live_count`.
    fn drive_surface_member_seam(host: &VerterHost, scope: &str) -> usize {
        let ctx: &dyn ResolverContext = host;
        let value_node =
            ctx.project_type_store()
                .semantic_graph()
                .intern_node(SemanticNodeData::new_typeof(
                    ValueRootKey {
                        scope: ScopeId {
                            canonical_id: Arc::from(scope),
                            local_scope: None,
                        },
                        name: Arc::from("definitelyMissingSeamValue"),
                    },
                    Arc::from(Vec::new().into_boxed_slice()),
                    Arc::from(Vec::new().into_boxed_slice()),
                ));
        let owner = DeclIdentity {
            canonical_id: Arc::from(scope),
            whole_hash: Default::default(),
            decl_name: Arc::from("<sfc-script-setup>"),
        };
        let projection = SurfaceProjection::whole_surface(PublishedSurfaceKind::Props);
        let member = shape_member("seamProbe", value_node);
        let admitted = AdmittedPublishedMember::admitted_for_test(
            owner,
            value_node,
            member,
            projection.cursor(),
            PublishedSurfaceKind::Props,
        );
        let mut engine = ComponentMetaQueryEngine::new(host);
        let _ = surface_member_to_expanded_field(
            &mut engine,
            scope,
            &admitted,
            None,
            None,
            None,
            MemberValuePosition::ShallowMember,
        );
        host.project_type_store().shape_cache_db().live_count()
    }

    // Control — an UNFENCED surface-member shape admits.
    let control = VerterHost::new_standalone(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        ..HostConfig::default()
    });
    let control_scope = "/fenced_surface/control.ts";
    upsert(&control, control_scope, "export type Probe = number;\n");
    control
        .ensure_indexed_ready(control_scope)
        .expect("control scope IndexedReady materialises");
    let control_before = control.project_type_store().shape_cache_db().live_count();
    let control_after = drive_surface_member_seam(&control, control_scope);
    assert!(
        control_after > control_before,
        "fixture invariant: an unfenced surface-member shape admits through \
         `surface_member_to_expanded_field` (otherwise the fenced assertion is vacuous)",
    );

    // Fenced — every `ensure_indexed_ready_serve` the compute drives is fenced at a
    // STABLE generation, so the shape is derived from a served-without-publication
    // artifact while its facts validate against the live view.
    let fenced = VerterHost::new_standalone(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        ..HostConfig::default()
    });
    let fenced_scope = "/fenced_surface/fenced.ts";
    upsert(&fenced, fenced_scope, "export type Probe = number;\n");
    fenced
        .ensure_indexed_ready(fenced_scope)
        .expect("fenced scope IndexedReady materialises");
    let fenced_before = fenced.project_type_store().shape_cache_db().live_count();
    let fenced_after = {
        let rctx = RequestContext::new(1, Arc::from(fenced_scope), false, None);
        let _guard = RequestContextGuard::install(rctx);
        fenced
            .test_force
            .force_indexed_ready_serve_fence_for_tests
            .store(true, Ordering::Relaxed);
        let after = drive_surface_member_seam(&fenced, fenced_scope);
        fenced
            .test_force
            .force_indexed_ready_serve_fence_for_tests
            .store(false, Ordering::Relaxed);
        // HARD FLOOR: a fenced serve is non-cacheable, NOT partial — the shape stays
        // Complete; non-cacheability routes through the fact tracer, never the sticky.
        assert!(
            !crate::request_context::current_request_result_is_partial(),
            "a fenced surface-member serve is non-cacheable, NOT partial — the shape stays \
             Complete; non-cacheability routes through the fact tracer, never the partial sticky",
        );
        after
    };
    assert_eq!(
        fenced_after, fenced_before,
        "POISON: a fenced (non-cacheable) surface-member shape was admitted into ShapeCacheDb \
         through `member_shape_peek_or_compute` — the nested fact tracer wrapping the cold reduce \
         must refuse admission, else a later same-generation warm hit inherits the stale shape \
         derived from a served-without-publication basis",
    );
}

/// The SAME surface-member `ShapeCacheDb` admission boundary must ALSO refuse on a
/// tracer `FactReadSetFinalise::Overflow` — the SECOND, independent non-admission
/// condition. The admit builds its signature from the carrier's `dep_signature`,
/// NOT from the cold-reduce tracer's finalised set, so an overflow seen only by the
/// tracer would be dropped on the floor and a ROOTLESS entry would warm the shared
/// cache: an observation set above `FACT_SIGNATURE_CAP` can be rooted by no
/// signature, so a warm read could never revalidate it.
///
/// DISCRIMINATING: the per-host overflow knob fans `FACT_SIGNATURE_CAP + 1`
/// synthetic observations into every installed tracer, so the member-shape compute's
/// tracer finalises `Overflow` with NO fenced serve and NO partial — the exact state
/// the pre-fix boundary (which read only `non_cacheable_read_observed` and discarded
/// the finalise) ADMITTED. The unarmed control admits (`live_count` grows); the
/// overflowed compute must NOT.
#[test]
fn tracer_overflow_refuses_surface_member_shape_admission() {
    use crate::meta_resolve::projection_demand::{PublishedSurfaceKind, SurfaceProjection};
    use crate::meta_resolve::projectors::output_sink::{
        surface_member_to_expanded_field, MemberValuePosition,
    };
    use crate::meta_resolve::projectors::publication_authority::AdmittedPublishedMember;
    use crate::request_context::{RequestContext, RequestContextGuard};
    use crate::resolver_core::ComponentMetaQueryEngine;
    use crate::semantic_query::{DeclIdentity, ScopeId, SemanticNodeData, ValueRootKey};
    use std::sync::atomic::Ordering;

    fn drive(host: &VerterHost, scope: &str) -> usize {
        let ctx: &dyn ResolverContext = host;
        let value_node =
            ctx.project_type_store()
                .semantic_graph()
                .intern_node(SemanticNodeData::new_typeof(
                    ValueRootKey {
                        scope: ScopeId {
                            canonical_id: Arc::from(scope),
                            local_scope: None,
                        },
                        name: Arc::from("definitelyMissingOverflowValue"),
                    },
                    Arc::from(Vec::new().into_boxed_slice()),
                    Arc::from(Vec::new().into_boxed_slice()),
                ));
        let owner = DeclIdentity {
            canonical_id: Arc::from(scope),
            whole_hash: Default::default(),
            decl_name: Arc::from("<sfc-script-setup>"),
        };
        let projection = SurfaceProjection::whole_surface(PublishedSurfaceKind::Props);
        let member = shape_member("overflowProbe", value_node);
        let admitted = AdmittedPublishedMember::admitted_for_test(
            owner,
            value_node,
            member,
            projection.cursor(),
            PublishedSurfaceKind::Props,
        );
        let mut engine = ComponentMetaQueryEngine::new(host);
        let _ = surface_member_to_expanded_field(
            &mut engine,
            scope,
            &admitted,
            None,
            None,
            None,
            MemberValuePosition::ShallowMember,
        );
        host.project_type_store().shape_cache_db().live_count()
    }

    // Control — an unarmed surface-member shape admits.
    let control = VerterHost::new_standalone(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        ..HostConfig::default()
    });
    let control_scope = "/overflow_surface/control.ts";
    upsert(&control, control_scope, "export type Probe = number;\n");
    control
        .ensure_indexed_ready(control_scope)
        .expect("control scope IndexedReady materialises");
    let control_before = control.project_type_store().shape_cache_db().live_count();
    let control_after = drive(&control, control_scope);
    assert!(
        control_after > control_before,
        "fixture invariant: an unarmed surface-member shape admits (otherwise the overflow \
         assertion is vacuous)",
    );

    // Overflowed — the shape compute's tracer observes above the cap.
    let host = VerterHost::new_standalone(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        ..HostConfig::default()
    });
    let scope = "/overflow_surface/overflowed.ts";
    upsert(&host, scope, "export type Probe = number;\n");
    host.ensure_indexed_ready(scope)
        .expect("scope IndexedReady materialises");
    let before = host.project_type_store().shape_cache_db().live_count();
    let after = {
        let rctx = RequestContext::new(1, Arc::from(scope), false, None);
        let _guard = RequestContextGuard::install(rctx);
        host.test_force
            .force_fact_tracer_overflow_observations
            .store(
                crate::resolver_core::FACT_SIGNATURE_CAP + 1,
                Ordering::Relaxed,
            );
        let after = drive(&host, scope);
        host.test_force
            .force_fact_tracer_overflow_observations
            .store(0, Ordering::Relaxed);
        // Orthogonality: overflow is non-cacheable, NOT partial.
        assert!(
            !crate::request_context::current_request_result_is_partial(),
            "a tracer overflow is non-cacheable, NOT partial — it must never raise the \
             request partial sticky",
        );
        after
    };
    assert_eq!(
        after, before,
        "POISON: a signature-OVERFLOWED member-shape compute was admitted into ShapeCacheDb — \
         an observation set above FACT_SIGNATURE_CAP can be rooted by no signature, so the \
         entry could never be revalidated on a warm read. Overflow must refuse INDEPENDENTLY \
         at this tracer boundary (pre-fix the `_finalise` was discarded)",
    );
}

/// (8b) Cross-view fail-closed. A member-value shape computed under a session
/// OVERLAY view is NOT served to the BASE view, and vice versa. Single-entry
/// displacement across incompatible views is acceptable; STALE SERVING across
/// views is not — the strict `ReadSetSignature` self-root validation roots the
/// entry on the view-authoritative content hash, so a read from the other view
/// recomputes cold.
///
/// Uses the real [`overlay_disc_fixture`] overlay (a base + overlay candidate
/// with distinct content hashes). The overlay-primed entry self-roots on the
/// overlay hash; the base view's `validates_self_root_whole_hash` reports the
/// BASE hash, so the base read mismatches and recomputes. The reverse
/// direction (base-primed → overlay read) mirrors it.
///
/// Each direction proves BOTH halves of the invariant, so neither a
/// never-store cache nor an always-store-but-stale-serve cache passes:
///
///  - **Same-view warm hit FIRST**: after priming under a view, a SECOND
///    read under the SAME view warm-hits — its cold closure PANICS if run,
///    and the warm value is the primed shape. This proves the entry was
///    ADMITTED and is REUSABLE in its own view (a cache that simply never
///    STORES overlay/base entries would FAIL this half).
///  - **Cross-view fail-closed SECOND**: only then does the OTHER view read;
///    its cold closure MUST run and surface its own recomputed shape (a
///    cache that stale-served across views would FAIL this half).
///
/// Direction 2 RE-PRIMES under the base explicitly (rather than relying on
/// direction 1's base recompute displacing the entry), so the reverse
/// fail-closed is asserted against a known-admitted base entry.
///
/// Discriminating property: each direction's same-view warm-hit assertion
/// trips RED if the entry was never admitted (never-store cache); each
/// cross-view "cold closure ran + the other view's value did NOT surface"
/// assertion trips RED if the slot stale-served across views.
#[test]
fn member_value_node_cross_view_fail_closed_recomputes() {
    use crate::component_meta_caches::ShapeCacheKey;
    use crate::resolver_core::SessionResolverContext;
    use crate::semantic_query::{ProjectionMode, ProjectionReductionContext, SemanticNodeId};

    let c = "/member_value_eq/cross_view.ts";
    let (host, view, base_hash, overlay_hash) = overlay_disc_fixture(c);

    // Build the overlay-aware request context (mirrors the existing
    // producer-overlay discrimination tests).
    let overlay_store_view = host
        .resolver_store_view_read()
        .into_owned_view()
        .with_session_overlay(&host, &view);
    let overlay_ctx = SessionResolverContext::new(
        &host,
        &view,
        &overlay_store_view,
        std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new()),
    );

    let base_ctx: &dyn ResolverContext = host.as_ref();
    let db = host.project_type_store().shape_cache_db();

    let value = SemanticNodeId(7);
    let mode = ProjectionMode::Expanded;
    let context = ProjectionReductionContext::published(mode);
    let member = shape_member("crossViewMember", value);
    let key = ShapeCacheKey::surface_member_value_whole_with_context_raw(
        Arc::<str>::from(c),
        &member,
        context,
    );

    // ---- Direction 1: prime under OVERLAY, read under BASE ----
    // The overlay-rooted entry self-roots on the overlay content hash. A
    // base read must NOT serve it: the base view's authoritative self-root
    // hash for `c` is the base hash, which mismatches the overlay-rooted
    // entry → fail-closed recompute.
    let _overlay_primed = db
        .get_or_compute_traced_for_test(&key, &overlay_ctx, || {
            Some((
                materialized("overlay-shape", empty_dep_signature()),
                self_root_at(c, overlay_hash),
            ))
        })
        .expect("overlay primes a member-value entry");

    // GAP-1 GUARD: prove the primed entry WARM-HITS in its OWN (overlay)
    // view BEFORE the cross-view read. A cache that simply never STORED
    // overlay entries would make the cross-view recompute below vacuous
    // (the base read would recompute because there is nothing to serve,
    // not because cross-view validation rejected an admitted entry). The
    // cold closure here PANICS — a warm hit must NOT run it — and the
    // returned value must be the overlay shape, proving admission + same-
    // view reuse.
    let overlay_same_view = db
        .get_or_compute_traced_for_test(&key, &overlay_ctx, || {
            panic!(
                "ADMISSION/REUSE BROKEN: the overlay-primed member-value entry did \
                 NOT warm-hit on a same-overlay-view read — its cold closure ran. \
                 The entry must be admitted under the overlay view and reused there, \
                 otherwise the cross-view recompute below is vacuous (nothing was \
                 ever stored to reject)."
            )
        })
        .expect("same-overlay-view read returns the primed value");
    assert!(
        matches!(&overlay_same_view.type_expr_for_test(), TypeExpr::Unknown { raw } if raw == "overlay-shape"),
        "the same-overlay-view warm hit must return the OVERLAY-primed shape \
         (`overlay-shape`), proving the entry was admitted under the overlay view",
    );

    let mut base_cold_ran = false;
    let base_read = db
        .get_or_compute_traced_for_test(&key, base_ctx, || {
            base_cold_ran = true;
            Some((
                materialized("base-shape", empty_dep_signature()),
                self_root_at(c, base_hash),
            ))
        })
        .expect("base read produces a value");
    assert!(
        base_cold_ran,
        "CROSS-VIEW STALE SERVE: a member-value shape computed under the OVERLAY view \
         was served to the BASE view. The entry self-roots on the overlay content hash; \
         the base view's strict self-root validation reports the base hash, so the base \
         read MUST mismatch and recompute cold — never warm-hit the overlay entry.",
    );
    assert!(
        matches!(&base_read.type_expr_for_test(), TypeExpr::Unknown { raw } if raw == "base-shape"),
        "the base read must surface its OWN recomputed shape (`base-shape`), never the \
         overlay entry's `overlay-shape`",
    );

    // ---- Direction 2: read under OVERLAY against a known-admitted BASE entry ----
    // GAP-2: make the reverse direction EXPLICIT rather than implicitly
    // relying on direction 1 having displaced the slot. Direction 1's base
    // read recomputed and ADMITTED a base-rooted entry (`base-shape`). Prove
    // that base entry is admitted + REUSABLE in its own (base) view BEFORE the
    // cross-view overlay read — a same-base read's cold closure must NOT run,
    // and it must surface the base value. This makes the overlay recompute
    // below reject an ADMITTED base entry rather than recompute from an empty
    // slot (which would make the reverse assertion vacuous).
    let mut base_same_view_cold_ran = false;
    let base_same_view = db
        .get_or_compute_traced_for_test(&key, base_ctx, || {
            base_same_view_cold_ran = true;
            Some((
                materialized("base-shape-unexpected", empty_dep_signature()),
                self_root_at(c, base_hash),
            ))
        })
        .expect("same-base-view read returns the admitted base value");
    assert!(
        !base_same_view_cold_ran,
        "ADMISSION/REUSE BROKEN (reverse): the base-rooted member-value entry \
         (admitted by direction 1's base recompute) did NOT warm-hit on a \
         same-base-view read — its cold closure ran. The base entry must be \
         admitted and reused in its own view, otherwise the reverse cross-view \
         recompute is vacuous.",
    );
    assert!(
        matches!(&base_same_view.type_expr_for_test(), TypeExpr::Unknown { raw } if raw == "base-shape"),
        "the same-base-view warm hit must return the admitted BASE shape \
         (`base-shape`, from direction 1's base recompute), proving the entry is \
         admitted + reusable under the base view",
    );

    // Symmetric cross-view: the base-rooted entry self-roots on the base
    // hash; the overlay read reports the overlay hash → mismatch → recompute.
    let mut overlay_cold_ran = false;
    let overlay_read = db
        .get_or_compute_traced_for_test(&key, &overlay_ctx, || {
            overlay_cold_ran = true;
            Some((
                materialized("overlay-shape-2", empty_dep_signature()),
                self_root_at(c, overlay_hash),
            ))
        })
        .expect("overlay read produces a value");
    assert!(
        overlay_cold_ran,
        "CROSS-VIEW STALE SERVE (reverse): a member-value shape rooted on the BASE \
         content hash was served to the OVERLAY view. The overlay view's strict \
         self-root validation reports the overlay hash, so the overlay read MUST \
         mismatch the base-rooted entry and recompute cold.",
    );
    assert!(
        matches!(&overlay_read.type_expr_for_test(), TypeExpr::Unknown { raw } if raw == "overlay-shape-2"),
        "the overlay read must surface its OWN recomputed shape (`overlay-shape-2`), \
         never a base-rooted entry's value",
    );

    // Fixture invariant: the two views genuinely disagree on the content
    // hash, otherwise the cross-view rejection would be vacuous.
    assert_ne!(
        base_hash, overlay_hash,
        "fixture invariant: base and overlay content hashes must differ for the \
         cross-view fail-closed test to discriminate",
    );
}

/// The `member_shape_peek_or_compute` GATE-SHORT-CIRCUIT arms (package-backed
/// root / transitive-cycle root / non-reducible shape) must REFUSE
/// [`crate::component_meta_caches::ShapeCacheDb`] admission when the arm's own
/// compute — the node-domain gates AND the terminal carrier raise — consumed a
/// FENCED (ReturnOnly, `store_published == false`) `IndexedReady` serve.
///
/// The three arms return BEFORE the cold-reduce fact tracer, so their admission
/// was gated only by `package_backed_fence_opt`, which is HASH-AVAILABILITY
/// (`authoritative_current_content_hash`), NOT publication status: a fenced serve
/// WITH an available content hash passes it. Each gate resolves the member value's
/// carrier head through the shared carrier resolver, which drives
/// `ensure_indexed_ready_serve` — so a fenced serve is genuinely consumed by the
/// gate whose verdict is admitted.
///
/// DISCRIMINATING, per arm: `force_indexed_ready_serve_fence_for_tests` fences
/// every `ensure_indexed_ready_serve` at a STABLE generation (no bump — so a
/// `GenerationSuperseded` gate cannot mask the refusal, and the served `indexed`
/// still resolves the shape). Anti-vacuity is asserted on BOTH halves: the unfenced
/// control ADMITS (`live_count` grows — the arm is a genuine admission path), and
/// the fenced drive genuinely OBSERVES a non-cacheable read (the tracer bit), so
/// the entry under assertion IS fenced-derived. The fenced drive must then leave
/// `live_count` unchanged.
#[cfg(test)]
mod fenced_gate_arm_admission_tests {
    use std::sync::atomic::Ordering;
    use std::sync::Arc;

    use verter_type_expr::{PrimitiveName, TypeExpr};

    use super::shape_member;
    use crate::meta_resolve::projection_demand::{PublishedSurfaceKind, SurfaceProjection};
    use crate::meta_resolve::projectors::output_sink::{
        surface_member_to_expanded_field, MemberValuePosition,
    };
    use crate::meta_resolve::projectors::publication_authority::AdmittedPublishedMember;
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    use crate::request_context::{RequestContext, RequestContextGuard};
    use crate::resolver_core::ComponentMetaQueryEngine;
    use crate::semantic_query::{
        DeclIdentity, NodeScopeId, ProjectionMode, SemanticNodeData, SemanticNodeId,
    };
    use crate::types::{AnalysisLevel, HostConfig};
    use crate::{DependencyResolution, VerterHost};

    /// Lower `expr` in `scope` to the settled graph node the per-member
    /// publication seam takes as `SurfaceMember.value`.
    fn lower(host: &VerterHost, scope: &str, expr: &TypeExpr) -> SemanticNodeId {
        ProjectSemanticDispatch::new(host)
            .lower_type_expr_in_scope_with_mode(scope, expr, ProjectionMode::Navigate)
            .expect("expr must lower")
    }

    /// Drive the REAL production per-member publication seam
    /// (`surface_member_to_expanded_field` -> `member_shape_peek_or_compute`) for
    /// `value_node` and return the post-drive `ShapeCacheDb` `live_count`.
    fn drive_member_seam(host: &VerterHost, scope: &str, value_node: SemanticNodeId) -> usize {
        let owner = DeclIdentity {
            canonical_id: Arc::from(scope),
            whole_hash: Default::default(),
            decl_name: Arc::from("<sfc-script-setup>"),
        };
        let projection = SurfaceProjection::whole_surface(PublishedSurfaceKind::Props);
        let member = shape_member("gateProbe", value_node);
        let admitted = AdmittedPublishedMember::admitted_for_test(
            owner,
            value_node,
            member,
            projection.cursor(),
            PublishedSurfaceKind::Props,
        );
        let mut engine = ComponentMetaQueryEngine::new(host);
        let _ = surface_member_to_expanded_field(
            &mut engine,
            scope,
            &admitted,
            None,
            None,
            None,
            MemberValuePosition::ShallowMember,
        );
        host.project_type_store().shape_cache_db().live_count()
    }

    /// Run the arm's fixture twice — an UNFENCED control host and a FENCED host —
    /// and assert the arm's fail-closed contract. `build` materialises the fixture
    /// on a fresh host and returns `(scope, member_value_node)`.
    fn assert_arm_refuses_fenced_admission<F>(arm: &str, build: F)
    where
        F: Fn(&VerterHost) -> (&'static str, SemanticNodeId),
    {
        // Control — the UNFENCED arm admits (otherwise the fenced assertion is vacuous).
        let control = VerterHost::new_standalone(HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        });
        let (control_scope, control_node) = build(&control);
        let control_before = control.project_type_store().shape_cache_db().live_count();
        let control_after = drive_member_seam(&control, control_scope, control_node);
        assert!(
            control_after > control_before,
            "fixture invariant ({arm}): the UNFENCED gate arm must ADMIT its shape into \
             ShapeCacheDb through `member_shape_peek_or_compute` — otherwise the fenced \
             assertion below is vacuous",
        );

        // Fenced — every `ensure_indexed_ready_serve` the arm's gates + raise drive is
        // fenced at a STABLE generation.
        let fenced = VerterHost::new_standalone(HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        });
        let (fenced_scope, fenced_node) = build(&fenced);
        let fenced_before = fenced.project_type_store().shape_cache_db().live_count();
        let rctx = RequestContext::new(1, Arc::from(fenced_scope), false, None);
        let _guard = RequestContextGuard::install(rctx);
        fenced
            .test_force
            .force_indexed_ready_serve_fence_for_tests
            .store(true, Ordering::Relaxed);
        let (fenced_after, read_set) =
            fenced.with_fact_tracer(|| drive_member_seam(&fenced, fenced_scope, fenced_node));
        fenced
            .test_force
            .force_indexed_ready_serve_fence_for_tests
            .store(false, Ordering::Relaxed);

        // Anti-vacuity: the arm's compute GENUINELY consumed a non-cacheable read, so
        // the entry under assertion is fenced-derived (not an unrelated admission).
        assert!(
            read_set.non_cacheable_read_observed(),
            "fixture invariant ({arm}): the fenced drive must genuinely consume a \
             NON-CACHEABLE read (the gate resolves the member value's carrier head through \
             `ensure_indexed_ready_serve`), else the refusal assertion below proves nothing",
        );
        // HARD FLOOR: a fenced serve is non-cacheable, NOT partial.
        assert!(
            !crate::request_context::current_request_result_is_partial(),
            "({arm}) a fenced serve is non-cacheable, NOT partial — the shape stays \
             Complete; non-cacheability routes through the fact tracer, never the sticky",
        );
        assert_eq!(
            fenced_after, fenced_before,
            "POISON ({arm}): a fenced (non-cacheable) gate-arm shape was ADMITTED into \
             ShapeCacheDb through `member_shape_peek_or_compute` — the arm must refuse \
             admission on the observed non-cacheable read, else a later same-generation \
             warm hit inherits the stale verdict derived from a served-without-publication \
             basis. `package_backed_fence_opt` is hash-AVAILABILITY, not publication status: \
             a fenced serve with an available hash passes it.",
        );
    }

    /// Arm 1 — the package-backed object-like ROOT gate.
    #[test]
    fn fenced_serve_package_backed_gate_member_shape_is_not_admitted() {
        assert_arm_refuses_fenced_admission("package-backed arm", |host| {
            const SCOPE: &str = "/src/pkg_arm.ts";
            let _ = host
                .upsert(crate::UpsertRequest {
                    canonical_id: Some("/src/node_modules/pkg/index.d.ts".to_string()),
                    input_id: "/src/node_modules/pkg/index.d.ts".to_string(),
                    source: Arc::from("export interface VendorProps { a: string; b: number }\n"),
                    file_language: crate::LanguageRegistry::global()
                        .classify_static("/src/node_modules/pkg/index.d.ts")
                        .static_resolution(),
                    aliases: Vec::new(),
                })
                .expect("package file upserts");
            super::upsert(
                host,
                SCOPE,
                "import type { VendorProps } from 'pkg'\nexport type Local = { x: string }\n",
            );
            host.set_import_dependencies(
                SCOPE,
                vec![DependencyResolution {
                    specifier: "pkg".to_string(),
                    resolved_canonical_id: Some("/src/node_modules/pkg/index.d.ts".to_string()),
                    possible_canonical_ids: Vec::new(),
                }],
            );
            host.ensure_indexed_ready(SCOPE).expect("scope indexes");
            (SCOPE, lower(host, SCOPE, &TypeExpr::named("VendorProps")))
        });
    }

    /// Arm 2 — the transitive-cycle generic-instantiation ROOT gate.
    #[test]
    fn fenced_serve_cycle_gate_member_shape_is_not_admitted() {
        assert_arm_refuses_fenced_admission("cycle arm", |host| {
            const SCOPE: &str = "/src/cycle_arm.ts";
            super::upsert(
                host,
                SCOPE,
                "export type A<T> = B<T>\nexport type B<T> = A<T>\n",
            );
            host.ensure_indexed_ready(SCOPE).expect("scope indexes");
            (
                SCOPE,
                lower(
                    host,
                    SCOPE,
                    &TypeExpr::named_with_args(
                        "A",
                        vec![TypeExpr::Primitive(PrimitiveName::String)],
                    ),
                ),
            )
        });
    }

    /// Arm 3 — the NON-REDUCIBLE stable-shape gate, on the ORDINARY production
    /// member value the macro hot mirror interns for `defineProps<{ msg: MyStr }>()`:
    /// an unresolved `BareRef` carrier with NO type arguments and NO reducible
    /// operator (so `classify_node_reduction_gates` clears both facts and the arm
    /// short-circuits). The package-backed gate resolves that carrier HEAD through
    /// the shared carrier resolver (`node_root_identity` -> `resolve_carrier_subject_node`),
    /// which rides `ensure_indexed_ready_serve` — so this arm's own verdict compute
    /// consumes the fenced serve it then admits under.
    #[test]
    fn fenced_serve_non_reducible_gate_member_shape_is_not_admitted() {
        assert_arm_refuses_fenced_admission("non-reducible arm", |host| {
            const SCOPE: &str = "/src/plain_arm.ts";
            super::upsert(host, SCOPE, "export type MyStr = string\n");
            let indexed = host.ensure_indexed_ready(SCOPE).expect("scope indexes");
            // The unresolved carrier the hot mirror mints — NOT a pre-resolved
            // `DeclRef` (which would carry its identity in-hand and need no head
            // resolution, so no serve would enter the gate).
            let node = host.project_type_store().semantic_graph().intern_node(
                SemanticNodeData::new_bare_ref(
                    Arc::from("MyStr"),
                    NodeScopeId::File {
                        canonical_id: Arc::from(SCOPE),
                        whole_hash: indexed.whole_hash,
                        local_scope: None,
                    },
                    Arc::from(Vec::new().into_boxed_slice()),
                ),
            );
            (SCOPE, node)
        });
    }
}

// ---------------------------------------------------------------------------
// Post-compute revalidation rejection — the winner RE-DERIVES; it is never
// served the value it built against a superseded read-set.
//
// TWO post-compute verdicts run on the single-entry funnels and they are NOT
// the same thing:
//
// - the CACHEABILITY verdict (`ComputedEntry::Unrooted`, the `CacheabilityProbe`):
//   the value IS a consistent snapshot of the view the compute ran under, it
//   simply cannot be ROOTED (an overflowed signature, an unobservable content
//   version, a fenced serve, a request-partial resolution). Only the WRITE is
//   refused — the value goes back to the winner through `ReturnOnly`, so the
//   refusal costs no second resolution.
// - the post-compute REVALIDATION verdict (`revalidate_after_compute`): the
//   store view MOVED under the compute — a file it read was edited, or the
//   project generation was reset, between its first read and the publish. Its
//   reads straddle the mutation, so the value is a consistent snapshot of NO
//   view. It is discarded and the caller RE-DERIVES against the fresh view (the
//   completion fence's retry-on-mid-flight-change). Serving it would hand the
//   caller a torn value AND bubble the superseded facts into the enclosing
//   entry's signature.
//
// The test pins the SECOND verdict on the FACT rail (a mid-compute content
// edit, at a STABLE project generation so the generation gate cannot be what
// rejects). It discriminates: a funnel that lowered the refused candidate back
// to the winner (the `lower_unadmitted` opt-in, which these nodes decline)
// would serve `"straddled"` instead of `None`, and the caller's re-derivation
// would never run.
// ---------------------------------------------------------------------------

/// `DeclarationLookupDb` — a cold compute whose read-set MOVED mid-flight is
/// REJECTED by post-compute revalidation, and the winner is NOT handed the
/// straddling value: the funnel returns `None` so the caller re-derives against
/// the fresh view.
///
/// DISCRIMINATING on three axes:
/// - the CONTROL (the same production signature, no mid-compute edit) ADMITS
///   and serves, so the rejection is caused by the edit and not by a fixture
///   signature the funnel could never validate;
/// - the project generation is STABLE across the edit, so the rejection is
///   provably the FACT rail (a `GenerationSuperseded` gate cannot mask it);
/// - the caller's re-derivation runs COLD and its FRESH value surfaces — the
///   rejected entry is neither served nor published.
#[test]
fn declaration_lookup_straddling_compute_is_not_served_to_the_winner() {
    use crate::resolver_core::component_meta_query_engine::engine_fact_signature_for_exported_type;

    let host = VerterHost::new_standalone(HostConfig::default());
    let ctx: &dyn ResolverContext = &host;
    let db = host.project_type_store().declaration_db();

    // CONTROL — a STABLE view: the same production signature admits and serves.
    let stable = "/reval_qdb/stable.ts";
    load_tracked_keyed(&host, stable);
    let stable_key = (Arc::<str>::from(stable), Arc::<str>::from("Probe"));
    let stable_observed = observed_whole_hash(ctx, stable);
    let control_live_before = db.live_count();
    let control = db
        .get_or_compute_traced_for_test(&stable_key, ctx, || {
            let sig =
                engine_fact_signature_for_exported_type(ctx, stable, "Probe", stable_observed)
                    .into_cacheable()
                    .expect(
                        "fixture invariant: the production signature builds for a tracked owner",
                    )
                    .facts;
            ComputedEntry::Rooted(decl("control", stable), sig)
        })
        .expect("the control cold publish succeeds");
    assert_eq!(
        control.text.as_deref(),
        Some("control"),
        "fixture invariant: with a STABLE view the same compute shape admits and serves its \
         value — so the rejection below is caused by the mid-compute edit alone",
    );
    assert!(
        db.live_count() > control_live_before,
        "fixture invariant: the control compute PUBLISHED — otherwise the no-publish assertion \
         below is vacuous",
    );

    // The STRADDLING compute: the owner is edited INSIDE the closure, after the
    // value was built and after its signature was rooted on the pre-edit
    // content version.
    let owner = "/reval_qdb/owner.ts";
    load_tracked_keyed(&host, owner);
    let key = (Arc::<str>::from(owner), Arc::<str>::from("Probe"));
    let observed = observed_whole_hash(ctx, owner);
    let generation_before = host.project_type_store().current_project_generation();
    let live_before = db.live_count();

    let outcome = db.get_or_compute_traced_for_test(&key, ctx, || {
        let sig = engine_fact_signature_for_exported_type(ctx, owner, "Probe", observed)
            .into_cacheable()
            .expect("fixture invariant: the production signature builds for a tracked owner")
            .facts;
        let entry = ComputedEntry::Rooted(decl("straddled", owner), sig);
        // The store view MOVES under the compute: the owner's content version is
        // no longer the one the value was read from.
        sibling_body_edit(&host, owner);
        entry
    });

    assert_eq!(
        host.project_type_store().current_project_generation(),
        generation_before,
        "fixture invariant: a content edit must NOT bump the project generation — the rejection \
         under test is the FACT rail, and a `GenerationSuperseded` gate must not be able to mask \
         it",
    );
    assert!(
        outcome.is_none(),
        "TORN SERVE: the funnel handed the winner the value it computed against a SUPERSEDED \
         read-set. A `revalidate_after_compute` rejection means the store view MOVED under the \
         compute — its reads straddle the mutation, so the value is a consistent snapshot of no \
         view at all. It must be discarded and the caller must re-derive against the fresh view \
         (the completion fence's retry-on-mid-flight-change). This is NOT the cacheability \
         refusal, which keeps a consistent-but-unrootable value and returns it through \
         `ReturnOnly`",
    );
    assert_eq!(
        db.live_count(),
        live_before,
        "a rejected cold compute publishes NO entry",
    );

    // The caller's re-derivation: it runs COLD (nothing warm survived the
    // rejection) and its FRESH value is what surfaces.
    let mut rederived = false;
    let fresh_observed = observed_whole_hash(ctx, owner);
    let fresh = db
        .get_or_compute_traced_for_test(&key, ctx, || {
            rederived = true;
            let sig = engine_fact_signature_for_exported_type(ctx, owner, "Probe", fresh_observed)
                .into_cacheable()
                .expect("fixture invariant: the production signature builds after the edit")
                .facts;
            ComputedEntry::Rooted(decl("rederived", owner), sig)
        })
        .expect("the re-derivation produces a value");
    assert!(
        rederived,
        "the re-derivation must run COLD — the rejected entry left nothing warm behind",
    );
    assert_eq!(
        fresh.text.as_deref(),
        Some("rederived"),
        "the caller-visible value must be the one re-derived against the FRESH view, never the \
         straddling value the funnel refused",
    );
}
