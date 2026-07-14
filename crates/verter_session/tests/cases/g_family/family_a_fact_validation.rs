//! R3/R26/R28 arch guard for the Family A inner caches. The
//! path-precise fact-dependency rail (`Arc<[FactVersionRef]>` + the
//! overflow flag) is now carried by the `ReadSetSignature` carrier,
//! and the four single-entry caches store their value + carrier in the
//! generic `cache_runtime::CacheEntry<V>` rather than a bespoke
//! per-cache `*Entry` struct.
//!
//! Live Family A fact-validated caches (the prepared-surface /
//! prepared-member / prepared-target / routed-expr caches and their
//! entries are DELETED — their absence is guarded by
//! `no_legacy_walker.rs::RETIRED_SYMBOLS`):
//!   - `ImportedRegistryDb` — its producer's transient
//!     `ImportedRegistryEntry` still carries
//!     `fact_dep_signature: Arc<[FactVersionRef]>`, lowered at
//!     admission to `CacheAdmission::Cacheable { signature:
//!     ReadSetSignature::new(...), self_root_canonicals,
//!     validated_at_generation }`.
//!   - `DeclarationLookupDb` / `ResolvabilityDb` / `OwnerCollectionDb`
//!     / `ShapeCacheDb` — each stores `Arc<CacheEntry<V>>` via the
//!     shared `SingleEntryArtifactNode` adapter. The carrier is
//!     `CacheEntry { signature: ReadSetSignature, self_root_canonicals,
//!     validated_at_generation }`.
//!
//! No cache entry may carry the legacy `dep_signature: DepSignature`
//! field. The warm-read validator routes through the
//! `ReadSetSignature::validate_with_self_roots(ctx, &self_roots)`
//! method (the strict self-root validator, passing the entry's keyed
//! canonical(s) as the self-root set) and the producer through the
//! live engine wrappers [`engine_fact_signature_for_exported_type`] /
//! [`engine_fact_signature_for_materialize_memo`] on cold compute.
//!
//! ## Source-grep arch guards
//!
//! The first test scans `component_meta_caches.rs` for the carrier
//! shapes (`Arc<CacheEntry<...>>` on the four single-entry caches,
//! `fact_dep_signature: Arc<[FactVersionRef]>` on the imported-registry
//! producer entry) and confirms the legacy field name is gone. The
//! second confirms the producer call-sites use the new
//! `engine_fact_signature_*` helpers (not the legacy
//! `engine_dep_signature_for_canonical`).

use std::fs;
use std::path::PathBuf;

fn read_session_source(relative: &str) -> String {
    let cargo_manifest_dir = env!("CARGO_MANIFEST_DIR");
    let mut path = PathBuf::from(cargo_manifest_dir);
    path.push("src");
    path.push(relative);
    fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()))
}

/// The Family A fact-validated caches carry the path-precise rail
/// through the `ReadSetSignature` carrier. The four single-entry caches
/// store `Arc<CacheEntry<V>>` (the carrier lives on `CacheEntry`); the
/// imported-registry producer entry still carries the raw
/// `fact_dep_signature: Arc<[FactVersionRef]>` it lowers at admission.
/// No cache entry carries the legacy `dep_signature: DepSignature`.
/// Source-grep arch guard.
#[test]
fn family_a_entries_carry_fact_dep_signature() {
    let src = read_session_source("component_meta_caches.rs");

    // 1. The four single-entry caches store their value + carrier in the
    //    generic `Arc<CacheEntry<V>>` rather than a bespoke `*Entry`
    //    struct. The carrier owns the `ReadSetSignature` rail. A
    //    regression that swapped the store back to a bespoke entry
    //    without the carrier would drop this field-shape and FAIL here.
    //    The walker cluster's `PreparedTargetEntry` / `PreparedSurfaceEntry`
    //    / `PreparedMemberEntry` / `RoutedExprSurfaceEntry` are DELETED;
    //    their absence is guarded by `no_legacy_walker.rs::RETIRED_SYMBOLS`.
    // The `OwnerCollectionDb` value migrated from the body-bearing
    // `Option<Arc<TypeExpr>>` to the content-free
    // `Option<AuthoredBodyLocator>` (a VALUE migration only — key and
    // validity oracle unchanged); its declaration wraps across lines, so the
    // pin matches the wrapped form.
    const SINGLE_ENTRY_STORES: &[&str] = &[
        "entries: DashMap<DeclarationLookupKey, Arc<CacheEntry<Arc<ResolvedTypeDeclaration>>>>",
        "entries: DashMap<ResolvabilityKey, Arc<CacheEntry<bool>>>",
        "entries: DashMap<\n        OwnerCollectionKey,\n        Arc<CacheEntry<Option<verter_type_expr::locators::AuthoredBodyLocator>>>,\n    >",
        "entries: DashMap<ShapeCacheKey, Arc<CacheEntry<MaterializedOutputTypeExpr>>>",
    ];
    for store in SINGLE_ENTRY_STORES {
        assert!(
            src.contains(store),
            "Family A single-entry cache must store `{store}` — the value + \
             `ReadSetSignature` carrier live in the generic `cache_runtime::CacheEntry<V>`. \
             A regression that reverted to a bespoke per-cache `*Entry` struct without \
             the carrier would drop the path-precise fact-validation rail."
        );
    }

    // 2. The `cache_runtime::CacheEntry` carrier owns the path-precise
    //    `signature: ReadSetSignature` rail every single-entry cache
    //    validates through. Pin its presence on the carrier definition —
    //    scoped STRICTLY to the `CacheEntry<V>` struct body. A file-wide
    //    `contains("pub signature: ReadSetSignature")` is NON-discriminating:
    //    the sibling `Candidate<D, V>` struct in the same file carries an
    //    identical `pub signature: ReadSetSignature` field, so dropping the
    //    carrier from `CacheEntry` while keeping `Candidate`'s would still
    //    pass file-wide. Windowing to the `CacheEntry<V>` body makes that
    //    drop flip the guard RED.
    let admission = read_session_source("cache_runtime/admission.rs");
    let cache_entry = struct_window(&admission, "pub(crate) struct CacheEntry<V> {");
    assert!(
        cache_entry.contains("pub signature: ReadSetSignature"),
        "`cache_runtime::CacheEntry<V>` must carry `signature: ReadSetSignature` — the \
         path-precise rail the four single-entry Family A caches validate against on \
         every warm hit. Dropping it would leave the stored entries with no observed \
         facts to revalidate. Window:\n{cache_entry}"
    );
    // Negative (scoped to the same `CacheEntry<V>` window): the carrier must
    // NOT regress to either legacy cache-validity rail. A file-wide negative
    // would false-match the materialiser carriers' explicitly-documented
    // non-validity `dispatch_dep_signature: DepSignature` field — so this is
    // window-scoped, mirroring the `ImportedRegistryEntry` negative below.
    assert!(
        !cache_entry.contains("dep_signature: DepSignature"),
        "`cache_runtime::CacheEntry<V>` must NOT carry the legacy \
         `dep_signature: DepSignature` cache-validity rail — the sole rail is the \
         `ReadSetSignature` carrier. A surviving legacy field would mean two coexisting \
         validity rails. Window:\n{cache_entry}"
    );
    assert!(
        !cache_entry.contains("fact_dep_signature: Arc<["),
        "`cache_runtime::CacheEntry<V>` must NOT carry the legacy \
         `fact_dep_signature: Arc<[FactVersionRef]>` raw rail — that transient producer \
         shape is lowered into the `ReadSetSignature` carrier at admission and must not \
         survive as a second stored validity rail on the entry. Window:\n{cache_entry}"
    );

    // 3. The imported-registry producer entry still carries the raw
    //    `fact_dep_signature: Arc<[FactVersionRef]>` it lowers at
    //    admission into `CacheAdmission::Cacheable { signature:
    //    ReadSetSignature::new(...), ... }`.
    let import_entry = struct_window(&src, "pub struct ImportedRegistryEntry {");
    assert!(
        import_entry.contains("fact_dep_signature: Arc<[FactVersionRef]>"),
        "ImportedRegistryEntry must carry `fact_dep_signature: Arc<[FactVersionRef]>` — \
         the producer's transient carrier lowered to a `ReadSetSignature` at admission. \
         Window:\n{import_entry}"
    );
    // Negative: the surviving entry struct must NOT carry the legacy
    //    `dep_signature: DepSignature` cache-validity rail. (The
    //    materialiser carriers DO keep a `dispatch_dep_signature:
    //    DepSignature` field, explicitly documented as NOT a
    //    cache-validity rail — so this negative is scoped to the entry
    //    struct window, never file-wide, to avoid false-matching that
    //    legitimate non-validity carrier.)
    assert!(
        !import_entry.contains("dep_signature: DepSignature"),
        "ImportedRegistryEntry must NOT carry the legacy `dep_signature: DepSignature` \
         cache-validity rail — the path-precise rail is the `ReadSetSignature` carrier. \
         A surviving legacy field would mean two coexisting validity rails. Window:\n{import_entry}"
    );

    // 4. The lowering site folds the producer entry's raw rail into the
    //    `ReadSetSignature` carrier at admission — pin the
    //    `CacheAdmission::Cacheable { signature: ReadSetSignature::new(...) }`
    //    shape so a regression that admitted without the carrier FAILS.
    assert!(
        src.contains("signature: crate::fact_signature_helpers::ReadSetSignature::new("),
        "the imported-registry admission lowering must wrap the producer's \
         `fact_dep_signature` into `ReadSetSignature::new(...)` on the \
         `CacheAdmission::Cacheable` arm. Admitting without the carrier would bypass \
         the path-precise validation rail."
    );
}

/// Extract the `pub struct NAME { … }` window — from the struct start to
/// the next `\n}` (column-0 struct close).
fn struct_window<'a>(src: &'a str, struct_decl: &str) -> &'a str {
    let idx = src
        .find(struct_decl)
        .unwrap_or_else(|| panic!("expected `{struct_decl}` in component_meta_caches.rs"));
    let after = &src[idx..];
    let end = after
        .find("\n}")
        .unwrap_or_else(|| panic!("expected struct close for `{struct_decl}`"));
    &after[..end]
}

/// The legacy `engine_dep_signature_for_canonical` helper is no
/// longer called by Family A producers. Per the R28 path-precise
/// contract, callers select one of:
/// - `engine_fact_signature_for_canonical_member` — for caches
///   keyed on a single member of an exporter type
///   (`MemberPresence + Member`).
/// - `engine_fact_signature_for_exported_type` — for caches keyed
///   on a top-level type identity
///   (`Export + LocalDecl + MemberShape`).
/// - `engine_fact_signature_for_materialize_memo` — for the
///   `MaterializeMemoDb` producer; provenance-pure, it roots the
///   keyed scope on the observed materialisation-time content hash
///   plus the observed-version `SyntacticExportSet` parse fact.
#[test]
fn family_a_producers_call_new_fact_helpers() {
    let registry =
        read_session_source("resolver_core/component_meta_query_engine/registry_decl.rs");
    assert!(
        !registry.contains("engine_dep_signature_for_canonical("),
        "registry_decl.rs must NOT call engine_dep_signature_for_canonical after the R28 \
         migration — use engine_fact_signature_for_exported_type instead."
    );

    // The four (canonical, name)-keyed shared-cache producers live in the
    // sibling `registry_cache_producers` module (they share one admission
    // discipline). Anti-vacuity first: the file this asserts on must really
    // own all four, so a producer that moved away can never leave the helper
    // assertion satisfied by an empty file.
    let producers = read_session_source(
        "resolver_core/component_meta_query_engine/registry_cache_producers.rs",
    );
    for producer in [
        "fn resolve_imported_registry_symbol(",
        "fn resolve_type_declaration(",
        "fn can_resolve_registry_symbol(",
        "fn owner_collection_expr(",
    ] {
        assert!(
            producers.contains(producer),
            "registry_cache_producers.rs must own the 4 (canonical, name)-keyed cache \
             producers (imported_registry_db, declaration_lookup_db, resolvability_db, \
             owner_collection_db); `{producer}` is missing. If a producer moved, this \
             guard must follow it — its fact-helper assertion is only meaningful on the \
             file that actually admits the entries."
        );
    }
    assert!(
        !producers.contains("engine_dep_signature_for_canonical("),
        "registry_cache_producers.rs must NOT call engine_dep_signature_for_canonical \
         after the R28 migration — use engine_fact_signature_for_exported_type instead."
    );
    assert!(
        producers.contains("engine_fact_signature_for_exported_type("),
        "registry_cache_producers.rs must call engine_fact_signature_for_exported_type for \
         its 4 (canonical, name)-keyed cache producers (imported_registry_db, \
         declaration_lookup_db, resolvability_db, owner_collection_db) — these track \
         top-level type identity."
    );

    let materialize = read_session_source("meta_resolve/materialize/field_types.rs");
    assert!(
        !materialize.contains("engine_dep_signature_for_canonical("),
        "meta_resolve/materialize/field_types.rs must NOT call \
         engine_dep_signature_for_canonical after the R28 migration — use \
         engine_fact_signature_for_materialize_memo for the materialize_memo_db producer."
    );
    assert!(
        materialize.contains("engine_fact_signature_for_materialize_memo("),
        "meta_resolve/materialize/field_types.rs must call \
         engine_fact_signature_for_materialize_memo for the materialize_memo_db \
         producer — it roots the keyed scope canonical AND merges every canonical \
         observed during materialization as a cross-file dependency fact."
    );
}

/// A shared validation adapter site and the exact set of
/// validation/bubble tokens its body carries on the warm-read path.
///
/// The Family A caches no longer each carry their own per-body
/// validate/bubble pair: the four single-entry caches
/// (`DeclarationLookupDb` / `ResolvabilityDb` / `OwnerCollectionDb` /
/// `ShapeCacheDb`) route their warm read through the SHARED
/// `SingleEntryArtifactNode::validate` + `single_entry_peek` adapters,
/// and the query-identity caches (`ImportedRegistryDb`,
/// `MaterializeStructureDb`) route through `QueryCandidateNode::lookup_candidate`
/// (plus the bespoke `ImportedRegistryDb::peek`). Each adapter body MUST
/// carry exactly one strict warm-read validator + one bubble, so
/// dropping the pair at any shared site flips the guard RED.
struct AdapterSiteSpec {
    /// Human-readable name of the adapter site (for panic messages).
    name: &'static str,
    /// The `fn` signature prefix that opens the adapter's body. Matched
    /// against `component_meta_caches.rs` (or, when `impl_anchor` is set,
    /// against that impl window so a sibling cache's same-named method
    /// cannot mask a drop here).
    fn_sig: &'static str,
    /// When set, scope the `fn_sig` search to this `impl XDb {` window
    /// first — needed for `ImportedRegistryDb::peek`, whose `pub(crate)
    /// fn peek(` signature is shared with `ShapeCacheDb::peek`.
    impl_anchor: Option<&'static str>,
}

/// The shared warm-read validation adapters every live Family A cache
/// routes through. The prepared-surface / prepared-member /
/// prepared-target / routed-expr DBs are DELETED (their absence is
/// guarded by `no_legacy_walker.rs::RETIRED_SYMBOLS`).
const ADAPTER_SITES: &[AdapterSiteSpec] = &[
    // The cooperative warm-hit validator shared by the four single-entry
    // caches (DeclarationLookupDb / ResolvabilityDb / OwnerCollectionDb /
    // ShapeCacheDb) through `SingleEntryArtifactNode`.
    AdapterSiteSpec {
        name: "SingleEntryArtifactNode::validate",
        fn_sig:
            "fn validate(\n        &self,\n        _key: &Self::Key,\n        entry: &CacheEntry",
        impl_anchor: None,
    },
    // The compute-once warm-read peek shared by the single-entry caches
    // that expose a `peek()` entry point (e.g. ShapeCacheDb).
    AdapterSiteSpec {
        name: "single_entry_peek",
        fn_sig: "fn single_entry_peek<K, V>(",
        impl_anchor: None,
    },
    // The warm-hit candidate validator shared by the query-identity
    // caches (ImportedRegistryDb / MaterializeStructureDb) through
    // `QueryCandidateNode`.
    AdapterSiteSpec {
        name: "QueryCandidateNode::lookup_candidate",
        fn_sig: "fn lookup_candidate(\n        &self,\n        key: &Self::Key,",
        impl_anchor: None,
    },
    // The bespoke compute-once peek on the imported-registry cache, which
    // validates a candidate inline rather than via `single_entry_peek`.
    AdapterSiteSpec {
        name: "ImportedRegistryDb::peek",
        fn_sig: "pub(crate) fn peek(",
        impl_anchor: Some("impl ImportedRegistryDb {"),
    },
];

/// A named Family A cache's routing method: the cold/warm entry point
/// that MUST construct the shared cache-runtime adapter node and hand it
/// to the shared `lookup` entry point. Pins that the cache routes through
/// the adapter rather than reading its backing store directly.
struct RoutingSiteSpec {
    /// Human-readable name (for panic messages).
    name: &'static str,
    /// The `impl XDb {` window to scope the routing-method search to, so a
    /// sibling cache's same-named method cannot mask a bypass here.
    impl_anchor: &'static str,
    /// The routing-method signature prefix opening the body to scan.
    fn_sig: &'static str,
    /// The shared adapter node type the routing body MUST construct —
    /// `SingleEntryArtifactNode` for the single-entry artifact caches or
    /// `QueryCandidateNode` for the two query-identity caches. The guard
    /// captures the `let <BIND> = <node_type> { … }` binding name and then
    /// asserts the SAME body passes that binding to the shared `lookup`
    /// (`lookup(&<BIND>,`). Matching by node type + captured binding is
    /// name-agnostic (a `node`→`adapter` rename still passes) and
    /// import-agnostic (any module-path prefix before `lookup` is allowed),
    /// while still discriminating a bypass: a cache that read `self.entries`
    /// / `self.store` directly never constructs the adapter node, so the
    /// capture fails and the guard goes RED.
    node_type: &'static str,
    /// The backing-store field the constructed adapter node MUST borrow —
    /// `entries: &self.entries,` / `store: &self.store,`. Pins that the
    /// adapter node is wired to the cache's real backing store rather than a
    /// detached throwaway.
    store_field: &'static str,
}

/// Every live Family A cache routes its warm/cold read through one of the
/// two shared cache-runtime adapter families:
/// - the four single-entry caches build a `SingleEntryArtifactNode {
///   entries: &self.entries, ... }` and hand it to the artifact-path
///   `lookup(&node, ...)`.
/// - the two query-identity caches build a `QueryCandidateNode { store:
///   &self.store, ... }` and hand it to `crate::cache_runtime::query::lookup`.
///
/// Pinning the adapter-node construction + the shared `lookup` call per
/// cache makes a regression that read the backing store directly (skipping
/// the adapter's strict warm-read validation) flip the guard RED.
const ROUTING_SITES: &[RoutingSiteSpec] = &[
    RoutingSiteSpec {
        name: "DeclarationLookupDb",
        impl_anchor: "impl DeclarationLookupDb {",
        fn_sig: "pub(crate) fn get_or_compute<F>(",
        node_type: "SingleEntryArtifactNode",
        store_field: "entries: &self.entries,",
    },
    RoutingSiteSpec {
        name: "ResolvabilityDb",
        impl_anchor: "impl ResolvabilityDb {",
        fn_sig: "pub(crate) fn get_or_compute<F>(",
        node_type: "SingleEntryArtifactNode",
        store_field: "entries: &self.entries,",
    },
    RoutingSiteSpec {
        name: "OwnerCollectionDb",
        impl_anchor: "impl OwnerCollectionDb {",
        fn_sig: "pub(crate) fn get_or_compute<F>(",
        node_type: "SingleEntryArtifactNode",
        store_field: "entries: &self.entries,",
    },
    RoutingSiteSpec {
        name: "ShapeCacheDb",
        impl_anchor: "impl ShapeCacheDb {",
        fn_sig: "pub(crate) fn get_or_compute<F>(",
        node_type: "SingleEntryArtifactNode",
        store_field: "entries: &self.entries,",
    },
    RoutingSiteSpec {
        name: "ImportedRegistryDb",
        impl_anchor: "impl ImportedRegistryDb {",
        fn_sig: "pub(crate) fn get_or_compute_admit<F>(",
        node_type: "QueryCandidateNode",
        store_field: "store: &self.store,",
    },
    RoutingSiteSpec {
        name: "MaterializeStructureDb",
        impl_anchor: "impl MaterializeStructureDb {",
        fn_sig: "pub(crate) fn get_or_compute_admit<F>(",
        node_type: "QueryCandidateNode",
        store_field: "store: &self.store,",
    },
];

/// The strict self-root validator method on the `ReadSetSignature`
/// carrier — the SOLE warm-read validity gate. The free-fn form
/// (`validate_fact_signature_with_self_roots`) is no longer called from
/// any Family A cache; the carrier method is.
const STRICT_VALIDATOR: &str = ".validate_with_self_roots(";
/// The fact-bubble method on the carrier — propagates the entry's
/// observed facts into the caller's outer tracer on a warm hit.
const BUBBLE: &str = ".bubble(";
/// The legacy lazy free-fn validator that routes a self-root
/// `FileWholeHash` through the untracked-accept rule. Forbidden.
const LAZY_VALIDATOR: &str = "validate_fact_signature(ctx,";

/// Extract the primary `impl XDb { … }` window — from `anchor` up to
/// the next top-level `\nimpl ` / `\npub struct ` / `\nstruct `,
/// exclusive.
fn extract_db_impl_window<'a>(src: &'a str, anchor: &str) -> &'a str {
    let start = src
        .find(anchor)
        .unwrap_or_else(|| panic!("expected `{anchor}` in component_meta_caches.rs"));
    let after = &src[start + anchor.len()..];
    let rel_end = ["\nimpl ", "\npub struct ", "\nstruct "]
        .iter()
        .filter_map(|m| after.find(m))
        .min()
        .unwrap_or(after.len());
    &after[..rel_end]
}

/// Assert that `region` (a named adapter site) carries EXACTLY one strict
/// validator and EXACTLY one bubble — a binary present/absent check at
/// that single site, not an aggregate `>= N` count.
fn assert_one_validator_one_bubble(region: &str, site: &str) {
    let validators = region.matches(STRICT_VALIDATOR).count();
    assert_eq!(
        validators, 1,
        "{site} MUST carry EXACTLY one `{STRICT_VALIDATOR}...)` — dropping it leaves a \
         stale-serve hole, duplicating it signals a mis-split. Observed {validators}. Region:\n{region}"
    );
    let bubbles = region.matches(BUBBLE).count();
    assert_eq!(
        bubbles, 1,
        "{site} MUST carry EXACTLY one `{BUBBLE}...)` so outer tracers see the inner \
         observation set at this site. Observed {bubbles}. Region:\n{region}"
    );
}

/// Every live Family A cache validates its warm-read path strictly
/// through the `ReadSetSignature::validate_with_self_roots(ctx,
/// &self_roots)` carrier method — the strict self-root validator: the
/// entry's keyed canonical(s) are passed as the self-root set, so the
/// leading self-root `FileWholeHash` is validated strictly (a
/// same-canonical edit, or a keyed canonical untracked by the live store
/// view, rejects the entry) while cross-file dependency facts keep lazy
/// permissiveness. The legacy lazy free-fn `validate_fact_signature`
/// warm-hit validator is forbidden.
///
/// BINARY per-named-SITE guard (NOT an aggregate `>= N` count): the four
/// caches no longer each carry their own validate/bubble pair — they
/// route through a small set of SHARED adapter bodies. The guard asserts
/// each shared adapter carries exactly one validator + one bubble, so
/// dropping the pair at any shared site (which would break every cache
/// routing through it) flips the guard RED. The windows are
/// fn-body-scoped, never file-wide, so a stray validator elsewhere can
/// never mask a dropped pair.
#[test]
fn family_a_warm_hit_uses_fact_validation() {
    let src = read_session_source("component_meta_caches.rs");

    for spec in ADAPTER_SITES {
        let search_scope: &str = match spec.impl_anchor {
            Some(anchor) => extract_db_impl_window(&src, anchor),
            None => &src,
        };
        let body = extract_fn_body(search_scope, spec.fn_sig);
        assert_one_validator_one_bubble(body, spec.name);
    }

    // Proving the SHARED adapters validate is necessary but NOT sufficient:
    // it does not prove each Family A cache actually ROUTES its warm read
    // through those adapters. A cache that read `self.entries` / `self.store`
    // directly — bypassing the adapter entirely — would still pass the
    // adapter-body checks above. So pin, per named cache, that its
    // cold/warm routing method constructs the shared adapter node and hands
    // it to the shared `lookup` entry point. Each window is the cache's
    // routing-method body (scoped to its own `impl XDb {`), so a sibling
    // cache's routing cannot mask a bypass here.
    //
    // The check is STRUCTURAL, not name-pinned: capture the `let <BIND> =
    // <node_type> { … }` binding identifier (regardless of the chosen name)
    // and then assert the SAME body hands THAT binding to the shared
    // `lookup(&<BIND>,` (regardless of any module-path prefix before
    // `lookup`). A harmless local rename (`node`→`adapter`) or a query
    // `lookup` import alias still passes; a bypass that reads `self.entries`
    // / `self.store` directly never constructs the adapter node, so the
    // capture fails and the guard goes RED — and a body that builds the node
    // but never passes it to `lookup` has no `lookup(&<BIND>,` and also goes
    // RED.
    let node_binding_re =
        regex::Regex::new(r"let\s+(\w+)\s*=\s*(SingleEntryArtifactNode|QueryCandidateNode)\s*\{")
            .expect("routing node-binding regex");
    for spec in ROUTING_SITES {
        let impl_window = extract_db_impl_window(&src, spec.impl_anchor);
        let body = extract_fn_body(impl_window, spec.fn_sig);

        // Capture the adapter-node binding for THIS spec's node type.
        let captured = node_binding_re
            .captures_iter(body)
            .find(|c| &c[2] == spec.node_type);
        let bind = captured.unwrap_or_else(|| {
            panic!(
                "{} MUST construct the shared cache-runtime adapter node \
                 `{} {{ … }}` in its `{}` body — a cache that read its backing \
                 store (`self.entries` / `self.store`) directly would carry no \
                 adapter-node construction and bypass the strict warm-read fact \
                 validation the adapter enforces. Body:\n{body}",
                spec.name, spec.node_type, spec.fn_sig,
            )
        });
        let bind = bind[1].to_string();

        // The constructed node must borrow the cache's real backing store.
        assert!(
            body.contains(spec.store_field),
            "{} adapter node `{}` must borrow its backing store (`{}`) in its \
             `{}` body. Body:\n{body}",
            spec.name,
            spec.node_type,
            spec.store_field,
            spec.fn_sig,
        );

        // The SAME body must hand THAT captured binding to the shared
        // `lookup`. The `lookup(&<BIND>,` substring is module-path-agnostic
        // (any prefix before `lookup` is allowed) and name-agnostic (the
        // captured binding, not a literal `node`).
        let lookup_call = format!("lookup(&{bind},");
        assert!(
            body.contains(&lookup_call),
            "{} MUST hand its adapter node `{bind}` to the shared `lookup` \
             entry point: its `{}` body must contain `{lookup_call}`. A body \
             that constructs the adapter node but never passes it to `lookup` \
             skips the shared strict warm-read fact validation. Body:\n{body}",
            spec.name,
            spec.fn_sig,
        );
    }

    // The cold winner bubbles + post-compute revalidates in the shared
    // `cache_runtime` substrate (`node.rs`). There are TWO cold-projection
    // sites — the artifact path (`fn lookup<N: ArtifactNode>`) and the
    // query-identity path (`fn lookup<N: QueryNode>`) — and each carries its
    // own cold-winner bubble closure + post-compute revalidator closure. A
    // file-wide `contains(STRICT_VALIDATOR)` / `contains(BUBBLE)` is
    // NON-discriminating: dropping the pair from EITHER cold path would still
    // pass because the OTHER path's pair satisfies the file-wide match. Scope
    // each cold-projection body separately and assert exactly-one validator +
    // exactly-one bubble in EACH (mirroring the warm-adapter exact-count
    // pattern), so dropping either pair flips the guard RED.
    let node_src = read_session_source("cache_runtime/node.rs");
    let artifact_lookup = extract_fn_body(&node_src, "pub(crate) fn lookup<N: ArtifactNode>(");
    assert_one_validator_one_bubble(artifact_lookup, "cache_runtime::node::lookup<ArtifactNode>");
    // The query-identity path carries TWO winner-side projections — the
    // admitted projection AND the admission-REFUSED opt-in projection
    // (`QueryNode::lower_unadmitted`): an admission-refused computed value
    // still flows to the winner, and its traced facts must STILL bubble
    // into the enclosing tracer so the rejected child's observations keep
    // rooting the consuming entries' signatures (cross-file invalidation).
    // Exactly ONE post-compute revalidator + exactly TWO bubbles: dropping
    // either projection's bubble (2→1) or both (2→0) flips the guard RED.
    let query_lookup = extract_fn_body(&node_src, "pub(crate) fn lookup<N: QueryNode>(");
    let query_validators = query_lookup.matches(STRICT_VALIDATOR).count();
    assert_eq!(
        query_validators, 1,
        "cache_runtime::node::query::lookup<QueryNode> MUST carry EXACTLY one \
         `{STRICT_VALIDATOR}...)` post-compute revalidator. Observed {query_validators}. \
         Region:\n{query_lookup}"
    );
    let query_bubbles = query_lookup.matches(BUBBLE).count();
    assert_eq!(
        query_bubbles, 2,
        "cache_runtime::node::query::lookup<QueryNode> MUST carry EXACTLY two \
         `{BUBBLE}...)` sites — one on the admitted winner projection, one on the \
         admission-REFUSED `lower_unadmitted` projection (the refused child's facts \
         must still root the enclosing signatures). Observed {query_bubbles}. \
         Region:\n{query_lookup}"
    );

    // The lazy free-fn `validate_fact_signature(ctx, …)` is forbidden
    // file-wide: it routes a self-root `FileWholeHash` through the
    // untracked-accept rule and would serve stale entries. Only the
    // strict carrier method is permitted for Family A caches.
    assert!(
        !src.contains(LAZY_VALIDATOR),
        "component_meta_caches.rs must NOT call the lazy `{LAZY_VALIDATOR} ...)` anywhere — \
         the lazy validator routes a self-root FileWholeHash through the untracked-accept \
         rule and serves stale entries. Use the strict \
         `ReadSetSignature::validate_with_self_roots(ctx, &self_roots)` carrier method \
         with the entry's keyed canonical(s) as the self-root set."
    );
}

/// Extract the body of the function whose signature begins at
/// `needle` in `src` — the brace-balanced span from the first `{`
/// after the signature to its matching `}`.
///
/// Brace-counting is robust against a nested column-0 `}` (e.g. a
/// `match`-arm block whose closing brace lands at column 0 inside the
/// function), which a first-`\n}` delimiter would mis-truncate.
/// String/char/comment literals containing stray braces are not a
/// concern here: the scanned functions are signature builders that
/// never embed `{`/`}` in a literal.
fn extract_fn_body<'a>(src: &'a str, needle: &str) -> &'a str {
    let start = src
        .find(needle)
        .unwrap_or_else(|| panic!("expected `{needle}` in source"));
    let after_sig = &src[start..];
    let open = after_sig
        .find('{')
        .unwrap_or_else(|| panic!("expected an opening brace for `{needle}`"));
    let bytes = after_sig.as_bytes();
    let mut depth = 0usize;
    let mut idx = open;
    while idx < bytes.len() {
        match bytes[idx] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return &after_sig[open..=idx];
                }
            }
            _ => {}
        }
        idx += 1;
    }
    panic!("expected a brace-balanced body for `{needle}`");
}

/// The central fact-signature helpers AND the engine wrappers that
/// delegate to them are **provenance-pure**: they root the keyed
/// canonical on a caller-supplied observed content hash, never a
/// current-content re-read. A current-content re-read inside a
/// signature builder reopens the publish race — an `upsert` landing
/// between a producer's value-compute and signature-build would root a
/// stale value on post-edit content, which then validates on warm
/// reads instead of missing.
///
/// This guard extracts each builder's brace-balanced function body and
/// asserts it calls NONE of the current-content-reading primitives —
/// any current-content re-read inside a signature builder MUST route
/// through one of these:
/// - `authoritative_current_content_hash` — the current-content
///   whole-hash oracle (the source the deleted `self_root_fact`
///   re-read helper used).
/// - `current_file_facts` — the current-content parse-fact reader
///   (the source the deleted `parse_fact_ref` re-read helper used).
/// - `parse_fact_ref(` — the deleted current-content parse-fact
///   builder (matched with its opening paren so it does not
///   false-match the provenance-pure
///   `parse_fact_ref_for_observed_current_content`).
/// - `shallow_file_state` — the base-host-only shallow-state oracle: a
///   producer that observes a self-root hash through it (a) re-reads
///   content rather than threading a provenance-observed hash, and (b)
///   under a `SessionResolverContext` reads the base file hash, not
///   the overlay's. A signature builder must NEVER observe a hash; it
///   takes the observed hash as a parameter.
///
/// The three central helpers live in `fact_signature_helpers.rs`; the
/// four `engine_fact_signature_for_*` wrappers live in the engine's
/// `mod.rs`. The producers (the observation point) are responsible for
/// read-ordering — not token-checkable; the producer-level
/// overlay-discrimination tests in `query_db_self_root_tests.rs` cover
/// that. Re-introducing any forbidden token inside any builder below
/// flips this guard RED.
#[test]
fn central_fact_signature_helpers_are_provenance_pure() {
    // Each token, if present in a builder body, reopens the publish
    // race. `parse_fact_ref(` is matched with its opening paren so it
    // does not false-match `parse_fact_ref_for_observed_current_content`.
    // `self_root_fact` is intentionally NOT listed: it is a deleted
    // symbol, and any re-read in its shape MUST consult
    // `authoritative_current_content_hash` — already banned below.
    const FORBIDDEN: &[&str] = &[
        "authoritative_current_content_hash",
        "current_file_facts",
        "parse_fact_ref(",
        "shallow_file_state",
    ];

    // The three central helpers in `fact_signature_helpers.rs`.
    let helpers_src = read_session_source("fact_signature_helpers.rs");
    const HELPERS: &[&str] = &[
        "pub(crate) fn fact_signature_for_exported_type(",
        "pub(crate) fn fact_signature_for_canonical_member(",
        "pub(crate) fn fact_signature_for_canonical_surface(",
    ];
    for helper in HELPERS {
        let body = extract_fn_body(&helpers_src, helper);
        for forbidden in FORBIDDEN {
            assert!(
                !body.contains(forbidden),
                "`{helper}` MUST NOT call `{forbidden}` — it is a current-content read \
                 and reopens the publish race the provenance-pure signature builders \
                 close. Root the keyed canonical on the caller-supplied observed hash \
                 and pin parse facts via `parse_fact_ref_for_observed_current_content` \
                 instead. Body:\n{body}"
            );
        }
    }

    // The live engine wrappers in `component_meta_query_engine/mod.rs`.
    // They delegate to the central helpers and must be provenance-pure
    // for the same reason — a re-read inside a wrapper is the same
    // publish-race hole as one inside the central helper. The
    // walker-cluster's `engine_fact_signature_for_prepared_target`
    // wrapper is DELETED (its `PreparedTargetDb` producer is gone), and
    // `engine_fact_signature_for_canonical_member` had no surviving
    // producer wrapper — the canonical-member signature builder lives in
    // `fact_signature_helpers.rs::fact_signature_for_canonical_member`,
    // already covered by the HELPERS list above.
    let engine_src = read_session_source("resolver_core/component_meta_query_engine/mod.rs");
    const ENGINE_WRAPPERS: &[&str] = &[
        "pub(crate) fn engine_fact_signature_for_exported_type(",
        "pub(crate) fn engine_fact_signature_for_materialize_memo(",
    ];
    for wrapper in ENGINE_WRAPPERS {
        let body = extract_fn_body(&engine_src, wrapper);
        for forbidden in FORBIDDEN {
            assert!(
                !body.contains(forbidden),
                "`{wrapper}` MUST NOT call `{forbidden}` — an engine signature wrapper \
                 must stay provenance-pure: it takes the observed content hash(es) as \
                 parameter(s) and delegates to the central helper, never re-reading \
                 current content. Body:\n{body}"
            );
        }
    }
}
