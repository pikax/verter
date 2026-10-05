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
//!
//! A third guard here used to scan `component_meta_caches.rs` for the
//! warm-read validation adapters and the per-cache routing bodies, plus the
//! `cache_runtime::node` cold-winner revalidators. Those bodies are owned by
//! the facade's own producers (`project_semantic_dispatch/memo.rs`,
//! `cache_runtime/node.rs`) and `component_meta_caches.rs` is passive storage,
//! so such a scanner would reject a storage split while proving nothing about
//! the strict warm-read contract. The contract itself is unchanged and
//! behaviourally owned by the fact matrix (`tests/cases/fact_matrix/`) and the
//! warm-hit cases that drive a real warm read against a live store view; the
//! compiler owns the single-definition property of the shared adapter bodies.

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

    // 4. The lowering site that folds the producer entry's raw rail into
    //    the `ReadSetSignature` carrier is NOT asserted from this file.
    //    `component_meta_caches.rs` is passive storage: the admission
    //    lowering and the warm-read validation adapters moved to the
    //    facade's own producers (`project_semantic_dispatch/memo.rs`), so
    //    scanning this storage file for the
    //    `CacheAdmission::Cacheable { signature: ReadSetSignature::new(...) }`
    //    shape rejected a storage split that changed nothing about the
    //    carrier. The carrier's behavioural contract is owned by the fact
    //    matrix (`tests/cases/fact_matrix/`) and the warm-hit cases that
    //    drive a real warm read against a live store view.
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

    let materialize = read_session_source("meta_resolve/projectors/output_sink.rs");
    assert!(
        !materialize.contains("engine_dep_signature_for_canonical("),
        "meta_resolve/projectors/output_sink.rs must NOT call \
         engine_dep_signature_for_canonical after the R28 migration — use \
         engine_fact_signature_for_materialize_memo for the materialize_memo_db producer."
    );
    assert!(
        materialize.contains("engine_fact_signature_for_materialize_memo("),
        "meta_resolve/projectors/output_sink.rs must call \
         engine_fact_signature_for_materialize_memo for the materialize_memo_db \
         producer — it roots the keyed scope canonical AND merges every canonical \
         observed during materialization as a cross-file dependency fact."
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
        "pub(crate) fn fact_signature_for_exported_type<",
        "pub(crate) fn fact_signature_for_canonical_member<",
        "pub(crate) fn fact_signature_for_canonical_surface<",
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
