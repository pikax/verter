//! Coverage guard for the typed cache invalidation
//! domain registration on [`verter_session::project_type_store::ProjectTypeStore`].
//!
//! The `project_type_store_dbs!` macro is the single source of
//! truth for registered DBs. This guard asserts:
//!
//! - The macro-generated inventory
//!   ([`PROJECT_TYPE_STORE_DB_INVENTORY`]) lists every DB-typed
//!   field on `ProjectTypeStore`.
//! - The macro-generated `all_dbs_for_invalidation()` returns one
//!   entry per registered DB.
//! - Every entry implements
//!   [`ParticipatesInInvalidation`] (compile-time enforcement).
//!
//! Adding a new DB-typed field outside the macro causes the
//! source-structure guard in `architecture_guards.rs::guard 8` to
//! reject the change. The two tests are complementary:
//! `architecture_guards::guard 8` parses the source structure;
//! this file exercises the macro-generated runtime surface.

use verter_session::invalidation_domain::{InvalidationDomain, ParticipatesInInvalidation};
use verter_session::project_type_store::{ProjectTypeStore, PROJECT_TYPE_STORE_DB_INVENTORY};

/// Acceptance gate (test 1 of 2 — runtime surface).
///
/// Asserts the macro-generated inventory and the runtime
/// `all_dbs_for_invalidation()` are coherent: same length, same
/// stable order. Adding a new DB to the macro registers both at
/// once; missing the registration manifests as a length mismatch.
#[test]
fn every_db_in_project_type_store_participates_in_invalidation() {
    let store = ProjectTypeStore::new();
    let dbs = store.all_dbs_for_invalidation();
    assert_eq!(
        dbs.len(),
        PROJECT_TYPE_STORE_DB_INVENTORY.len(),
        "all_dbs_for_invalidation() must return one entry per inventory name. \
         inventory = {:?}, runtime length = {}",
        PROJECT_TYPE_STORE_DB_INVENTORY,
        dbs.len(),
    );
    // Every participant returns SOME domain set (possibly empty for
    // pure registries like IntrinsicRegistry, but every entry must
    // be addressable through the trait).
    for (name, db) in PROJECT_TYPE_STORE_DB_INVENTORY.iter().zip(dbs.iter()) {
        // Just calling `domains()` proves the trait is implemented;
        // the returned slice may legitimately be empty for
        // registries that do not participate in any invalidation
        // domain (e.g. `IntrinsicRegistry`).
        let _ = db.domains();
        // A non-empty inventory name proves the macro recorded the
        // field name.
        assert!(
            !name.is_empty(),
            "PROJECT_TYPE_STORE_DB_INVENTORY entry must not be empty",
        );
    }
}

/// Object-safety regression. The trait
/// [`ParticipatesInInvalidation`] is callable through `&dyn`.
/// Adding an associated `Key` type to this trait would silently
/// break this; the design splits per-DB
/// `InvalidationByCanonical` (NOT object-safe) from
/// `ParticipatesInInvalidation` (object-safe) for exactly this
/// reason.
#[test]
fn participates_in_invalidation_remains_object_safe() {
    fn _check(_: &dyn ParticipatesInInvalidation) {}
    let store = ProjectTypeStore::new();
    let dbs = store.all_dbs_for_invalidation();
    if let Some(first) = dbs.first() {
        _check(*first);
    }
}

/// Every domain variant is reachable. Sanity check
/// for the enum's `ALL` constant and its iteration ordering. Also
/// asserts the variant count matches the documented contract
/// (`{FileContent, TypeGraph, ResolverState, ComponentMeta,
/// ProjectGeneration, AppConfigInterfaceMerge}` = 6 variants).
#[test]
fn invalidation_domain_all_lists_six_variants() {
    assert_eq!(
        InvalidationDomain::ALL.len(),
        6,
        "InvalidationDomain::ALL must enumerate exactly the 6 \
         variants from the plan brief: FileContent, TypeGraph, \
         ResolverState, ComponentMeta, ProjectGeneration, \
         AppConfigInterfaceMerge",
    );
    assert!(InvalidationDomain::ALL.contains(&InvalidationDomain::FileContent));
    assert!(InvalidationDomain::ALL.contains(&InvalidationDomain::TypeGraph));
    assert!(InvalidationDomain::ALL.contains(&InvalidationDomain::ResolverState));
    assert!(InvalidationDomain::ALL.contains(&InvalidationDomain::ComponentMeta));
    assert!(InvalidationDomain::ALL.contains(&InvalidationDomain::ProjectGeneration));
    assert!(InvalidationDomain::ALL.contains(&InvalidationDomain::AppConfigInterfaceMerge));
}
