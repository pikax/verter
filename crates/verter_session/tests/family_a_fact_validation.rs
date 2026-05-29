//! R3/R26/R28 arch guard for the Family A inner caches.
//!
//! The eight single-entry caches (`DeclarationLookupDb`,
//! `ResolvabilityDb`, `OwnerCollectionDb`, `PreparedTargetDb`,
//! `ShapeCacheDb`, `PreparedSurfaceDb`, `PreparedMemberDb`,
//! `RoutedExprSurfaceDb`) store the shared cache-runtime carrier
//! `cache_runtime::CacheEntry<Value>` and route their cold builds
//! through `cache_runtime::lookup` — the validity rails (the fact
//! signature, the self-root canonicals, the compute-time generation)
//! live on the shared entry, NOT on a bespoke per-cache `*Entry`. The
//! one remaining bespoke carrier is `ImportedRegistryEntry`, the
//! `QueryNode`-bound imported-registry cache, which still carries
//! `fact_dep_signature: Arc<[FactVersionRef]>` until its own migration.
//!
//! The producer-side helpers (`engine_fact_signature_for_*` and the
//! central `fact_signature_for_*` helpers) are unchanged and still
//! drive the cold-compute signature build; the producer guards below
//! pin them.

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

/// `ImportedRegistryEntry` — the one remaining bespoke Family A carrier
/// (the `QueryNode`-bound imported-registry cache, migrated separately) —
/// carries `fact_dep_signature: Arc<[FactVersionRef]>` and never the
/// legacy `dep_signature: DepSignature`. Source-grep arch guard.
#[test]
fn imported_registry_entry_carries_fact_dep_signature() {
    let src = read_session_source("component_meta_caches.rs");
    let struct_decl = "pub struct ImportedRegistryEntry {";
    let idx = src
        .find(struct_decl)
        .unwrap_or_else(|| panic!("expected `{struct_decl}` in component_meta_caches.rs"));
    let after = &src[idx..];
    let end = after
        .find("\n}")
        .expect("expected struct close for ImportedRegistryEntry");
    let window = &after[..end];
    assert!(
        window.contains("fact_dep_signature: Arc<[FactVersionRef]>"),
        "ImportedRegistryEntry must carry `fact_dep_signature: Arc<[FactVersionRef]>` \
         (R28), but the struct body did not contain that field. Window:\n{window}"
    );
    assert!(
        !window.contains("dep_signature: DepSignature"),
        "ImportedRegistryEntry must NOT carry the legacy `dep_signature: DepSignature` \
         field. Window:\n{window}"
    );
}

/// The eight single-entry Family A caches store the shared cache-runtime
/// carrier `Arc<CacheEntry<Value>>` and define NO bespoke per-cache
/// `*Entry` struct. The validity rails (fact signature, self-root
/// canonicals, compute-time generation) live on the shared entry. A
/// regression reintroducing a bespoke carrier — or storing a non-runtime
/// entry type — fails here.
#[test]
fn single_entry_caches_store_cache_runtime_entry() {
    let src = read_session_source("component_meta_caches.rs");
    // (Db name, the exact `entries: DashMap<...>` value type the
    // migrated cache must store.)
    const MIGRATED: &[(&str, &str)] = &[
        (
            "DeclarationLookupDb",
            "Arc<CacheEntry<Arc<ResolvedTypeDeclaration>>>",
        ),
        ("ResolvabilityDb", "Arc<CacheEntry<bool>>"),
        (
            "OwnerCollectionDb",
            "Arc<CacheEntry<Option<Arc<TypeExpr>>>>",
        ),
        // PreparedTargetDb's value is the `PreparedTargetValue` alias
        // (`Option<(Arc<str>, Arc<str>)>`) — a `type` alias keeps the
        // map signature within clippy's type-complexity bound.
        ("PreparedTargetDb", "Arc<CacheEntry<PreparedTargetValue>>"),
        ("ShapeCacheDb", "Arc<CacheEntry<MaterializedTypeExpr>>"),
        (
            "PreparedSurfaceDb",
            "Arc<CacheEntry<PreparedSurfacePayload>>",
        ),
        (
            "PreparedMemberDb",
            "Arc<CacheEntry<Option<Arc<ProjectedMember>>>>",
        ),
        ("RoutedExprSurfaceDb", "Arc<CacheEntry<Arc<TypeExpr>>>"),
    ];
    for (db, value_type) in MIGRATED {
        assert!(
            src.contains(value_type),
            "{db} must store `{value_type}` (the shared cache-runtime carrier) in its \
             `entries` map. A bespoke per-cache `*Entry` carrier violates the cutover."
        );
    }
    // None of the eight migrated caches define a bespoke carrier struct.
    for retired in [
        "pub struct DeclarationLookupEntry",
        "pub struct ResolvabilityEntry",
        "pub struct OwnerCollectionEntry",
        "pub struct PreparedTargetEntry",
        "pub struct ShapeCacheEntry",
        "pub struct PreparedSurfaceEntry",
        "pub struct PreparedMemberEntry",
        "pub struct RoutedExprSurfaceEntry",
    ] {
        assert!(
            !src.contains(retired),
            "the bespoke carrier `{retired}` MUST be retired — the migrated single-entry \
             caches store `cache_runtime::CacheEntry<Value>`."
        );
    }
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
    assert!(
        registry.contains("engine_fact_signature_for_exported_type("),
        "registry_decl.rs must call engine_fact_signature_for_exported_type for its 4 \
         (canonical, name)-keyed cache producers (imported_registry_db, declaration_lookup_db, \
         resolvability_db, owner_collection_db) — these track top-level type identity."
    );

    let prepared_surface =
        read_session_source("resolver_core/component_meta_query_engine/prepared_surface.rs");
    assert!(
        !prepared_surface.contains("engine_dep_signature_for_canonical("),
        "prepared_surface.rs must NOT call engine_dep_signature_for_canonical after the R28 \
         migration — use the engine_fact_signature_* helpers instead."
    );
    // PreparedSurface observes top-level identity directly;
    // PreparedTarget observes top-level identity for both keyed
    // canonicals via the engine_fact_signature_for_prepared_target
    // helper; PreparedMember observes per-member facts.
    assert!(
        prepared_surface.contains("engine_fact_signature_for_exported_type("),
        "prepared_surface.rs must call engine_fact_signature_for_exported_type for the \
         prepared_surface_db cache producer (top-level identity)."
    );
    assert!(
        prepared_surface.contains("engine_fact_signature_for_prepared_target("),
        "prepared_surface.rs must call engine_fact_signature_for_prepared_target for the \
         prepared_target_db cache producer — it roots BOTH the active scope and the \
         declaring canonical as self-roots."
    );
    assert!(
        prepared_surface.contains("engine_fact_signature_for_canonical_member("),
        "prepared_surface.rs must call engine_fact_signature_for_canonical_member for the \
         prepared_member_db cache producer (path-precise member observation per R28)."
    );

    let routed_expr =
        read_session_source("resolver_core/component_meta_query_engine/routed_expr.rs");
    assert!(
        !routed_expr.contains("engine_dep_signature_for_canonical("),
        "routed_expr.rs must NOT call engine_dep_signature_for_canonical after the R28 \
         migration — use engine_fact_signature_for_exported_type instead."
    );
    assert!(
        routed_expr.contains("engine_fact_signature_for_exported_type("),
        "routed_expr.rs must call engine_fact_signature_for_exported_type for the \
         routed_expr_surface_db cache producer (top-level identity)."
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

/// Family A warm reads validate strictly against the entry's own
/// self-roots and never through the lazy validator.
///
/// After the single-entry cutover, the eight migrated caches validate
/// through the shared cache-runtime entry's
/// `ReadSetSignature::validate_with_self_roots(ctx,
/// &entry.self_root_canonicals)` (in the `SingleEntryArtifactNode`
/// adapter's `validate` and in `single_entry_peek`), and bubble through
/// `ReadSetSignature::bubble`. `ImportedRegistryEntry` (still bespoke)
/// validates through the free `validate_fact_signature_with_self_roots`.
/// The contract this guard pins is boundary-and-behavioural, not a
/// call-site count: strict self-root validation everywhere, the lazy
/// `validate_fact_signature(ctx, ...)` nowhere.
#[test]
fn family_a_warm_hit_uses_fact_validation() {
    let src = read_session_source("component_meta_caches.rs");

    // The migrated single-entry caches validate through the shared
    // cache-runtime entry's strict self-root validator. The adapter's
    // `validate` passes `cx.resolver`; the shared `single_entry_peek`
    // passes `ctx`. Both route into `ReadSetSignature::validate_with_self_roots`
    // against the entry's OWN `self_root_canonicals` (whitespace-robust
    // substring — the receiver is `entry.signature` / `entry_arc.signature`).
    assert!(
        src.contains(".validate_with_self_roots(cx.resolver, &entry.self_root_canonicals)"),
        "the migrated single-entry adapter MUST validate through \
         `entry.signature.validate_with_self_roots(cx.resolver, &entry.self_root_canonicals)`."
    );
    assert!(
        src.contains(".validate_with_self_roots(ctx, &entry_arc.self_root_canonicals)"),
        "`single_entry_peek` MUST validate through \
         `entry_arc.signature.validate_with_self_roots(ctx, &entry_arc.self_root_canonicals)`."
    );
    // And bubble through the same carrier so outer tracers observe the
    // entry's facts (warm hits AND the cold winner's projection).
    assert!(
        src.contains("entry.signature.bubble(cx.resolver)")
            && src.contains("entry_arc.signature.bubble(ctx)"),
        "the migrated single-entry caches MUST bubble through \
         `entry.signature.bubble(...)` so outer tracers observe the entry's facts."
    );

    // `ImportedRegistryEntry` (still bespoke until its own migration)
    // keeps the free strict self-root validator.
    assert!(
        src.contains("validate_fact_signature_with_self_roots("),
        "ImportedRegistryEntry's warm-read / revalidation closures MUST validate via \
         `validate_fact_signature_with_self_roots(...)` (strict self-root)."
    );

    // The lazy `validate_fact_signature(ctx, ...)` must NOT appear for
    // ANY Family A cache — it routes a self-root `FileWholeHash` through
    // the untracked-accept rule and would serve a stale entry. This ban
    // is the load-bearing invariant; it must hold regardless of how the
    // validation is centralised.
    assert!(
        !src.contains("validate_fact_signature(ctx,"),
        "component_meta_caches.rs must NOT call the lazy `validate_fact_signature(ctx, \
         ...)` for any Family A cache — only strict self-root validation is permitted."
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

    // The four engine wrappers in `component_meta_query_engine/mod.rs`.
    // They delegate to the central helpers and must be provenance-pure
    // for the same reason — a re-read inside a wrapper is the same
    // publish-race hole as one inside the central helper.
    let engine_src = read_session_source("resolver_core/component_meta_query_engine/mod.rs");
    const ENGINE_WRAPPERS: &[&str] = &[
        "pub(crate) fn engine_fact_signature_for_exported_type(",
        "pub(crate) fn engine_fact_signature_for_canonical_member(",
        "pub(crate) fn engine_fact_signature_for_prepared_target(",
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
