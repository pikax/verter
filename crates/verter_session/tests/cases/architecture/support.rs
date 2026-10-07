// Architecture enforcement guards. Fail when known rules are broken.
// Cheap static source scans, run on every change.

use std::fs;
use std::path::PathBuf;

pub(super) fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("repo root")
        .to_path_buf()
}

pub(super) fn read_workspace_file(rel: &str) -> String {
    fs::read_to_string(workspace_root().join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"))
}

pub(super) fn workspace_path(rel: &str) -> std::path::PathBuf {
    workspace_root().join(rel)
}

/// Count occurrences of any of the supplied needles in `src`.
/// Returns the total number of byte-substring hits across all needles.
pub(super) fn count_callsites(src: &str, needles: &[&str]) -> usize {
    needles.iter().map(|n| src.matches(n).count()).sum()
}

// ----------------------------------------------------------------------
// Phase 8 — `no_off_store_host_caches`
//
// Static guard for the "no caches outside ProjectTypeStore" architectural
// rule documented in CLAUDE.md ("Project-global cache (final state)") and
// re-stated by the universal preamble R4 of the architecture-cutover plan
// ("Do not add caches outside ProjectTypeStore. Do not add request-local
// mirrors of host state.").
//
// Phase 6b's classification of every cache-shaped `VerterHost` field is
// the binding source of truth. Each `legitimate-authority` field is
// recorded in `phase_8_allow_list()` with the §6b sub-plan citation and
// the architectural rationale. The `mirror`-classified route fields (F3
// routes/imported_roots and F7 route_owned_shallow_cache) were already
// deleted by Phase 6b — this guard verifies their continued absence.
//
// Body design (per §8.1 of the cutover plan):
//   1. Parse `crates/verter_session/src/lib.rs` via `syn::parse_file`.
//   2. Locate `pub struct VerterHost`.
//   3. For each field, render the type signature to a string and
//      classify by structural shape:
//        - `DashMap<...>`, `Shared<FxHashMap...>`,
//          `Mutex<...>`, `RwLock<...>` (with or without a
//          `parking_lot::` qualifier) → cache-shape candidate.
//        - `Atomic*`, `ArcSwap*`, simple `Arc<T>`, `Box<T>`,
//          plain owned types → non-cache shape, PASS.
//   4. For each cache-shape candidate, assert it is either:
//        - on the allow-list below (with phase-report citation), OR
//        - the `project_type_store` field itself (the destination).
//   5. Re-verify that the deleted mirror field names do not reappear.
//
// Phase 8 ships this guard un-ignored: Phase 8 has full visibility into
// the post-rehoming shape and is the sole author of this discipline.
// Future commits that add a new cache-shaped field on `VerterHost` MUST
// either rehome the field into `ProjectTypeStore` or extend the
// allow-list with a phase-report citation justifying the exception.
//
// SCOPE GAP (TODO U10): `is_cache_shape` inspects only the TOP-LEVEL rendered
// field type, so a cache family whose `DashMap`s are nested inside a named
// struct held behind `Arc<...>` is NOT surveyed. Two such off-`ProjectTypeStore`
// families exist today and pass this guard without an allow-list entry:
//   - `framework_script_caches: Arc<FrameworkScriptCaches>` (a content-addressed
//     candidate store + a fact store), and
//   - the `FrameworkSurfaceStore`s reached through
//     `framework_registry: Arc<FrameworkAdapterRegistry>`.
// Both are fact-validated (correct today). They are PROVISIONAL and are
// consolidated onto `ProjectTypeStore` at block U10; when rehomed, this gap
// closes (or the deepened survey added by U10 covers them directly).

/// The Phase-8 allow-list for `no_off_store_host_caches`. Each entry is a
/// `VerterHost` cache-shape field that Phase 6b classified as
/// `legitimate-authority`, paired with a one-line phase-report citation
/// and architectural rationale.
pub(super) fn phase_8_allow_list() -> std::collections::HashMap<&'static str, &'static str> {
    [
        // (a) Cache-shape fields explicitly classified by Phase 6b as
        //     legitimate-authority — see phase-06b-report.md and the
        //     phase-06b sub-plan §6b.2.
        (
            "alias_to_canonical",
            "phase-06b-report.md §F12: caller-supplied virtual-alias map populated at upsert time, disjoint from VFS overlay and ProjectResolver. Host-scoped, no equivalent in ProjectTypeStore.",
        ),
        (
            "last_const_prop_overrides",
            "phase-06b-report.md §F13: Phase-7 invalidation state-diff record (NOT a cache of resolution results). No equivalent in ProjectTypeStore.",
        ),
        // F1, F2, F4, F5 — rehomed in Tier 1C-α (host-cache-rehoming.md
        // §3.4 + plan §3.4.1). The four fields (`compile_cache`,
        // `resolved_type_cache`, `eval_env_cache`, `semantic_db`) no
        // longer live on `VerterHost`; the syn-walk that drives this
        // allow-list will not surface them. Re-adding any of them to
        // `VerterHost` would fail this guard until a fresh
        // rehoming-doc rationale is added.
        (
            "query_profile",
            "phase-06b-report.md §F10: execution-policy state, not a result memoiser. Different artifact type than anything in ProjectTypeStore.",
        ),
        // (b) Single-cell handles whose `RwLock<Arc<dyn>>` shape matches
        //     the cache-detection pattern but whose semantics are
        //     config-handle, not hashmap-cache. Documented here for
        //     completeness so the guard's allow-list captures every
        //     deviation.
        (
            "workspace",
            "phase-06b-report.md §6b.2.F6.bypass: single-cell workspace handle (Arc<RwLock<Arc<dyn WorkspaceAccess>>>) shared with the scheduler's SourceLoader so the lock always reads through the latest workspace after set_workspace(). NOT a cache; a re-pointable handle.",
        ),
        // (c2) Test-only concurrency seams for mid-flight mutation tests.
        //     `#[cfg(test)] materialize_seam_hook` is a single-cell
        //     `Mutex<Option<Arc<dyn Fn()>>>` hook slot fired inside the
        //     `IndexedReady` materialise flights (base / edge-refresh /
        //     overlay) so mid-flight mutation tests can park a flight
        //     deterministically between its generation-stamp capture and
        //     its pre-publish fence. Compiled out in production builds.
        (
            "materialize_seam_hook",
            "Pre-publish-fence regression pins: Mutex<Option<Arc<dyn Fn()>>> test-only seam slot fired inside materialise flights for deterministic mid-flight mutation tests. Compiled out in production builds. NOT a cache.",
        ),
        // - `flight_retry_seam_hook` (NOT a cache):
        //     `#[cfg(test)] flight_retry_seam_hook` is the sibling
        //     single-cell hook slot fired inside the `ensure_indexed_ready_serve`
        //     singleflight retry loop (after a follower records a fenced
        //     outcome) so sustained-churn tests can interleave a fresh
        //     leader + mutation per bounded attempt. Compiled out in
        //     production builds.
        (
            "flight_retry_seam_hook",
            "Sustained-churn ReturnOnly regression pin: Mutex<Option<Arc<dyn Fn()>>> test-only seam slot fired inside the singleflight retry loop for deterministic sustained-churn choreography. Compiled out in production builds. NOT a cache.",
        ),
        // - `compile_publish_seam_hook` (NOT a cache):
        //     `#[cfg(test)] compile_publish_seam_hook` is the sibling
        //     single-cell hook slot fired inside `get_virtual_file`'s
        //     cold compile path (after the compile, before the
        //     mode-routed publish) so fence tests can land an env /
        //     project mutation deterministically in the compute→publish
        //     window. Compiled out in production builds.
        (
            "compile_publish_seam_hook",
            "Content-mode compile pre-publish-fence regression pin (env_mutation_between_compute_and_publish_declines_the_content_publish): Mutex<Option<Arc<dyn Fn()>>> test-only seam slot fired in the compute→publish window for deterministic mid-flight mutation choreography. Compiled out in production builds. NOT a cache.",
        ),
        // - `compile_input_seam_hook` (NOT a cache):
        //     `#[cfg(test)] compile_input_seam_hook` is the sibling
        //     single-cell hook slot fired inside `get_virtual_file`'s
        //     cold compile path (after the request's source snapshot
        //     is captured, before the compile input is assembled) so
        //     fence tests can land a content mutation
        //     deterministically in the snapshot→compile-input window.
        //     Compiled out in production builds.
        (
            "compile_input_seam_hook",
            "Content-mode compile snapshot-coherence regression pin (content_mutation_between_snapshot_and_compile_input_never_publishes_under_the_stale_hash): Mutex<Option<Arc<dyn Fn()>>> test-only seam slot fired in the snapshot→compile-input window for deterministic mid-flight mutation choreography. Compiled out in production builds. NOT a cache.",
        ),
        // - `edge_refresh_gate_seam_hook` (NOT a cache):
        //     `#[cfg(test)] edge_refresh_gate_seam_hook` is the sibling
        //     single-cell hook slot fired inside `ensure_indexed_ready_serve`'s
        //     singleflight body (after the edge-refresh parse-env reuse
        //     gate passes, before the refresh flight runs) so fence tests
        //     can land a parse-env-moving mutation deterministically in
        //     the reuse-gate→publish window. Compiled out in production
        //     builds.
        (
            "edge_refresh_gate_seam_hook",
            "Edge-refresh parse-env fence regression pin (parse_env_mutation_between_reuse_gate_and_refresh_declines_the_edge_refresh_publish): Mutex<Option<Arc<dyn Fn()>>> test-only seam slot fired in the reuse-gate→refresh window for deterministic mid-flight mutation choreography. Compiled out in production builds. NOT a cache.",
        ),
        // - `raw_snapshot_template_join_seam_hook` (NOT a cache):
        //     `#[cfg(test)] raw_snapshot_template_join_seam_hook` is the
        //     sibling single-cell hook slot fired inside the
        //     raw-analysis-snapshot scheduler lane (after the lane's
        //     analysis snapshot capture, before the template-analysis
        //     source join) so fence tests can land a content upsert
        //     deterministically in the capture→join window. Compiled
        //     out in production builds.
        (
            "raw_snapshot_template_join_seam_hook",
            "Raw-analysis-snapshot template-join fence regression pin (source_move_between_analysis_capture_and_template_join_never_persists_the_template): Mutex<Option<Arc<dyn Fn()>>> test-only seam slot fired in the analysis-capture→template-source-join window for deterministic mid-flight mutation choreography. Compiled out in production builds. NOT a cache.",
        ),
        // - `template_persist_seam_hook` (NOT a cache):
        //     `#[cfg(test)] template_persist_seam_hook` is the sibling
        //     single-cell hook slot fired inside the lazy
        //     template-analysis computation (after the by-value inputs
        //     produced the template, before the `derived_raw_cache`
        //     persist) so fence tests can land a content upsert
        //     deterministically in the compute→persist window. Compiled
        //     out in production builds.
        (
            "template_persist_seam_hook",
            "Template-slot generation-rail regression pin (source_move_between_compute_and_persist_never_serves_the_stale_template): Mutex<Option<Arc<dyn Fn()>>> test-only seam slot fired in the compute→persist window for deterministic mid-flight mutation choreography. Compiled out in production builds. NOT a cache.",
        ),
        // - `narrowed_scope_serve_seam_hook` (NOT a cache):
        //     `#[cfg(test)] narrowed_scope_serve_seam_hook` is the
        //     sibling single-cell hook slot fired inside
        //     `get_analysis_snapshot_internal`'s narrowed-scope serve
        //     branch (after the branch's source snapshot capture,
        //     before its snapshot products assembly) so fence tests can
        //     land a content upsert deterministically in the
        //     capture→assembly window. Compiled out in production
        //     builds.
        (
            "narrowed_scope_serve_seam_hook",
            "Narrowed-scope single-generation snapshot regression pin (source_move_inside_the_narrowed_scope_window_never_serves_a_generation_mix): Mutex<Option<Arc<dyn Fn()>>> test-only seam slot fired in the source-capture→products-assembly window for deterministic mid-flight mutation choreography. Compiled out in production builds. NOT a cache.",
        ),
        // - `compile_blockers_serve_seam_hook` (NOT a cache):
        //     `#[cfg(test)] compile_blockers_serve_seam_hook` is the
        //     sibling single-cell hook slot fired inside
        //     `get_compile_blockers` (after the source snapshot
        //     capture, before its snapshot products assembly) so fence
        //     tests can land a content upsert deterministically in the
        //     capture→assembly window. Compiled out in production
        //     builds.
        (
            "compile_blockers_serve_seam_hook",
            "Compile-blockers single-generation snapshot regression pin (source_move_inside_the_compile_blockers_window_never_serves_a_generation_mix): Mutex<Option<Arc<dyn Fn()>>> test-only seam slot fired in the source-capture→products-assembly window for deterministic mid-flight mutation choreography. Compiled out in production builds. NOT a cache.",
        ),
        // - `indexed_source_capture_seam_hook` (NOT a cache):
        //     `#[cfg(test)] indexed_source_capture_seam_hook` is the
        //     sibling single-cell hook slot fired inside the base
        //     `IndexedReady` materialise flight (after the source
        //     snapshot is held, before remaining IndexedReady products
        //     are assembled from that object) so fence tests can land a
        //     content upsert deterministically in that window. Compiled
        //     out in production builds.
        (
            "indexed_source_capture_seam_hook",
            "IndexedReady snapshot-coherence regression pin (source_move_between_parse_facts_and_eval_source_never_serves_torn_identity): Mutex<Option<Arc<dyn Fn()>>> test-only seam slot fired after the source snapshot is held so every content-addressed IndexedReady product stays one snapshot object. Compiled out in production builds. NOT a cache.",
        ),
        // - `parse_env_override` (NOT a cache):
        //     `#[cfg(test)] parse_env_override` is a single-cell
        //     `Mutex<Option<Hash16>>` test-only override of the live
        //     parse-env dimension read by `host_view_env_hashes(_for)`.
        //     The production parse dimension derives solely from the
        //     constant workspace parser flags, so fence tests flip this
        //     override (paired with a `project_generation` bump) to
        //     emulate a parse-env-moving configuration change mid-flight.
        //     Compiled out in production builds.
        (
            "parse_env_override",
            "Edge-refresh parse-env fence regression pin (parse_env_mutation_between_reuse_gate_and_refresh_declines_the_edge_refresh_publish): Mutex<Option<Hash16>> test-only live parse-env dimension override read by host_view_env_hashes(_for). Compiled out in production builds. NOT a cache; a per-host single-cell test knob.",
        ),
        // (d) Typeinfo scratch synthesis cache (§5 Phase 3) — per-host
        //     LRU of synthesised scratch URI → SemanticNodeId. The
        //     cache lives on `VerterHost` (not ProjectTypeStore)
        //     because scratch URIs are session-local synthesis artefacts
        //     gated by `cacheable: bool` per-request, not project-wide
        //     resolution results. Configurable capacity via
        //     `HostConfig::typeinfo_scratch_cache_capacity` (default 64).
        (
            "typeinfo_scratch_cache",
            "§5.3 / Phase 3: per-host LRU mapping scratch URI → SemanticNodeId for `evaluate_type_expression(cacheable: true)`. Session-local synthesis cache, not a project-state result memoiser; ProjectTypeStore is for cross-request project-wide results.",
        ),
    ]
    .into_iter()
    .collect()
}

/// Classify a rendered type signature by structural shape. Returns true
/// for cache-shape candidates (DashMap / Shared<HashMap> / Mutex<...> /
/// RwLock<...>). Returns false for `Arc<T>`, `Box<T>`, `Atomic*`, owned
/// scalars, and other non-cache-shape types.
///
/// The substring matches are deliberately broad: any field whose type
/// signature contains `Mutex<` or `RwLock<` or `DashMap<` or
/// `Shared<FxHashMap` or `Shared<HashMap` is treated as a cache-shape
/// candidate. The token-stream renderer used by `render_type` emits both
/// `Mutex <` (with the angle-bracket-padding syn produces) and `Mutex<`
/// (after string normalisation), so we match both forms defensively.
pub(super) fn is_cache_shape(rendered_ty: &str) -> bool {
    let r = rendered_ty;
    r.contains("DashMap <")
        || r.contains("DashMap<")
        || r.contains("Shared < FxHashMap")
        || r.contains("Shared<FxHashMap")
        || r.contains("Shared < HashMap")
        || r.contains("Shared<HashMap")
        || r.contains("Mutex <")
        || r.contains("Mutex<")
        || r.contains("RwLock <")
        || r.contains("RwLock<")
}

/// Render a `syn::Type` to a string via its `ToTokens` impl. Stable
/// across rustc versions because syn's token stream emission is
/// canonical (single-space-separated tokens).
pub(super) fn render_type(ty: &syn::Type) -> String {
    use quote::ToTokens;
    let tokens = ty.to_token_stream();
    tokens.to_string()
}

/// The core algorithm of `no_off_store_host_caches`. Given a parsed
/// `pub struct <ident> { ... }` named-fields struct and the allow-list,
/// returns `(violations, surveyed_cache_fields)`. Pure function — no
/// I/O — so it is reusable by the discriminator self-test.
pub(super) fn no_off_store_host_caches_inner(
    parsed: &syn::File,
    target_struct: &str,
    allow_list: &std::collections::HashMap<&str, &str>,
) -> (Vec<String>, Vec<(String, String)>) {
    use syn::{Fields, Item};
    let mut violations = Vec::<String>::new();
    let mut surveyed_cache_fields = Vec::<(String, String)>::new();
    let mut found_struct = false;
    for item in &parsed.items {
        let Item::Struct(s) = item else { continue };
        if s.ident != target_struct {
            continue;
        }
        found_struct = true;
        let Fields::Named(named) = &s.fields else {
            panic!(
                "{target_struct} is expected to have named fields; found {:?}",
                s.fields
            );
        };
        for field in &named.named {
            let field_name = field
                .ident
                .as_ref()
                .map(|id| id.to_string())
                .unwrap_or_default();
            let rendered_ty = render_type(&field.ty);
            // Skip the project_type_store destination field itself.
            // It is an Arc<ProjectTypeStore> — by structural shape it
            // is `Arc<...>`, not a cache pattern, but we name it
            // explicitly so the guard's intent is unambiguous.
            if field_name == "project_type_store" {
                continue;
            }
            if !is_cache_shape(&rendered_ty) {
                continue;
            }
            surveyed_cache_fields.push((field_name.clone(), rendered_ty.clone()));
            // Check allow-list.
            if allow_list.contains_key(field_name.as_str()) {
                continue;
            }
            // Check whether the type signature points at
            // ProjectTypeStore (i.e., the field IS a ProjectTypeStore
            // handle even though its type contains RwLock/Mutex/DashMap
            // by happenstance). The integration tip has no such field
            // today; this branch is a forward-looking allowance for
            // fields like `cache_root: Arc<ProjectTypeStore>` if a
            // future commit restructures the host.
            if rendered_ty.contains("ProjectTypeStore") {
                continue;
            }
            violations.push(format!(
                "{target_struct}::{field_name}: cache-shape field of type \
                 `{rendered_ty}` is neither on the documented allow-list \
                 (with a phase-report citation) nor on ProjectTypeStore. \
                 Either rehome into ProjectTypeStore (preferred per \
                 CLAUDE.md \"Project-global cache (final state)\" and \
                 plan R4) or extend this guard's allow-list with a \
                 phase-report citation justifying the exception."
            ));
        }
        break;
    }
    assert!(
        found_struct,
        "no_off_store_host_caches: did not find `pub struct {target_struct}` \
         in the parsed file — guard cannot verify the post-Phase-6b shape."
    );
    (violations, surveyed_cache_fields)
}

// ===========================================================================
// guard 8 — every DB-typed field on `ProjectTypeStore` appears in the
// inventory `PROJECT_TYPE_STORE_DB_INVENTORY` and the runtime
// `all_dbs_for_invalidation()` list. Plan §12.A3 / §12.A10 step 7.
//
// The inventory is the single source of truth for which DBs participate
// in the typed cache invalidation cascade. Adding a DB-typed field
// outside the inventory fails this guard.
//
// Companion runtime guard:
// `crates/verter_session/tests/cases/g_misc0/invalidation_coverage.rs`'s
// `every_db_in_project_type_store_participates_in_invalidation` walks
// the macro-generated runtime surface; this source-structure guard
// walks the actual struct definition and asserts every DB-typed field
// appears in the inventory.
// ===========================================================================

/// Predicate: does `rendered_ty` syntactically look like one of the
/// host-owned DB / Store / Registry types tracked by
/// [`crate::project_type_store::ProjectTypeStore`]?
///
/// Recognizes the suffix-pattern `*Db`, `*Store`, `*Registry`, plus
/// generic forms wrapping the same suffixes, plus `Arc<...>` wrappers.
/// Tolerant of syn's whitespace canonicalization (single-space-
/// separated tokens).
pub(super) fn is_db_shape(rendered_ty: &str) -> bool {
    // Strip `Arc <` / `Arc<` wrapper before pattern-matching the
    // inner type's suffix.
    let inner = rendered_ty
        .trim()
        .strip_prefix("Arc <")
        .or_else(|| rendered_ty.trim().strip_prefix("Arc<"))
        .unwrap_or(rendered_ty)
        .trim_end_matches('>')
        .trim();
    // Recognize the head identifier: take chars up to `<` or
    // whitespace.
    let head_end = inner
        .find(|c: char| c == '<' || c.is_whitespace())
        .unwrap_or(inner.len());
    let head = inner[..head_end].trim();
    // The DB suffix family. `Counters` / `Snapshot` / `Hash` etc.
    // are NOT DB-shape and are excluded by the strict suffix check.
    let suffixes = ["Db", "Store", "Registry"];
    suffixes
        .iter()
        .any(|suf| head.ends_with(suf) && head.len() > suf.len())
}

/// Walk `source` (a `syn::parse_file`-able Rust file) for the struct
/// named `struct_ident` and return the names of every field whose type
/// matches a DB-shape pattern (`*Db`, `*Store`, `*Registry`,
/// `Arc<*Db>`, `Arc<*Store>`, `Arc<*Registry>`,
/// `ComponentMetaResultDb<...>`).
///
/// Returns the names that are NOT in `registered`. Pure function for
/// the deliberate-violation test below.
pub(super) fn unregistered_db_fields_in_struct(
    source: &str,
    struct_ident: &str,
    registered: &[&str],
) -> Vec<String> {
    use syn::{parse_file, Item};

    let parsed = parse_file(source).expect("parse source via syn");
    let mut unregistered: Vec<String> = Vec::new();

    for item in &parsed.items {
        let Item::Struct(item_struct) = item else {
            continue;
        };
        if item_struct.ident != struct_ident {
            continue;
        }
        let syn::Fields::Named(named) = &item_struct.fields else {
            continue;
        };
        for field in &named.named {
            let Some(field_name) = field.ident.as_ref() else {
                continue;
            };
            let field_name_str = field_name.to_string();
            let rendered_ty = render_type(&field.ty);
            if !is_db_shape(&rendered_ty) {
                continue;
            }
            if !registered.iter().any(|r| *r == field_name_str) {
                unregistered.push(field_name_str);
            }
        }
    }

    unregistered
}

// ===========================================================================
// guard 9 — every DB-typed field on `ProjectTypeStore` has a
// corresponding `impl InvalidationByCanonical for ...` block somewhere
// in the verter_session crate sources. Plan §12.A12 acceptance gate.
//
// Source-structure guard. Walks the struct via `syn::parse_file` and
// extracts the head identifier of each DB-shape field's type
// (`FileArtifactStore`, `AnalysisReadyDb`, `RouteDb`, ...). For every
// such head identifier, asserts that at least one source file under
// `crates/verter_session/src/` contains an
// `impl ... InvalidationByCanonical for <Head>` block.
//
// Companion runtime guard:
// `crates/verter_session/tests/cases/g_misc0/invalidation_perf.rs`'s
// `invalidate_canonical_touches_only_indexed_entries` exercises the
// O(K) drain semantics for one representative DB; this guard asserts
// the full inventory is uniformly covered.
// ===========================================================================

/// Extract the head identifier of every DB-shape field's type from
/// `source` (a `syn::parse_file`-able Rust file) for the struct named
/// `struct_ident`. Strips `Arc<...>` and generic-parameter forms so
/// `Arc<RouteDb>` and `ComponentMetaResultDb<T>` both reduce to their
/// head identifier.
pub(super) fn db_field_type_heads_in_struct(source: &str, struct_ident: &str) -> Vec<String> {
    use syn::{parse_file, Item};

    let parsed = parse_file(source).expect("parse source via syn");
    let mut heads: Vec<String> = Vec::new();

    for item in &parsed.items {
        let Item::Struct(item_struct) = item else {
            continue;
        };
        if item_struct.ident != struct_ident {
            continue;
        }
        let syn::Fields::Named(named) = &item_struct.fields else {
            continue;
        };
        for field in &named.named {
            if field.ident.is_none() {
                continue;
            }
            let rendered_ty = render_type(&field.ty);
            if !is_db_shape(&rendered_ty) {
                continue;
            }
            // Reduce to head identifier — same logic as `is_db_shape`'s
            // internal head extraction.
            let inner = rendered_ty
                .trim()
                .strip_prefix("Arc <")
                .or_else(|| rendered_ty.trim().strip_prefix("Arc<"))
                .unwrap_or(&rendered_ty)
                .trim_end_matches('>')
                .trim();
            let head_end = inner
                .find(|c: char| c == '<' || c.is_whitespace())
                .unwrap_or(inner.len());
            let head = inner[..head_end].trim().to_string();
            if !heads.contains(&head) {
                heads.push(head);
            }
        }
    }

    heads
}

/// Search every `.rs` file under `crates/verter_session/src/` for a
/// `impl ... InvalidationByCanonical for <type_head>` block. Tolerant
/// of `impl crate::invalidation_domain::InvalidationByCanonical`,
/// `impl<P> crate::...InvalidationByCanonical for ComponentMetaResultDb<P>`,
/// and the bare `impl InvalidationByCanonical for ...` form.
pub(super) fn invalidation_by_canonical_impl_exists(
    crate_root: &std::path::Path,
    type_head: &str,
) -> bool {
    use std::fs;

    fn walk(dir: &std::path::Path, files: &mut Vec<std::path::PathBuf>) {
        let Ok(read_dir) = fs::read_dir(dir) else {
            return;
        };
        for entry in read_dir.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, files);
            } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
                files.push(path);
            }
        }
    }

    let mut files = Vec::new();
    walk(crate_root, &mut files);

    // Two variants matched: `for <Head>` (concrete) and `for <Head><`
    // (generic). The pattern allows arbitrary whitespace and the
    // optional `crate::invalidation_domain::` prefix.
    let needle_concrete = format!("InvalidationByCanonical for {type_head}");
    let needle_generic = format!("InvalidationByCanonical for {type_head}<");
    let needle_concrete_eol = format!("InvalidationByCanonical for {type_head}\n");
    let needle_concrete_brace = format!("InvalidationByCanonical for {type_head} ");

    for file in files {
        let Ok(src) = fs::read_to_string(&file) else {
            continue;
        };
        // A path-qualified trait (`verter_type_engine::invalidation_domain::
        // InvalidationByCanonical`) is long enough that rustfmt breaks the
        // header before `for`; match the header with whitespace runs collapsed
        // to one space so the line break does not hide the impl.
        let collapsed = src.split_whitespace().collect::<Vec<_>>().join(" ");
        if src.contains(&needle_generic)
            || src.contains(&needle_concrete_eol)
            || src.contains(&needle_concrete_brace)
            || src.contains(&format!("{needle_concrete}\r\n"))
            || src.contains(&format!("{needle_concrete}{{"))
            || collapsed.contains(&needle_generic)
            || collapsed.contains(&needle_concrete_brace)
            || collapsed.contains(&format!("{needle_concrete}{{"))
        {
            return true;
        }
    }
    false
}

/// Architecture guard: every bump of the `inflight_aborted_retries`
/// and `cold_aborts_swept` counters in
/// `crates/verter_type_engine/src/semantic_query_memo/mod.rs` must go
/// through the `record_inflight_aborted_retry` /
/// `record_cold_abort_swept` helpers. Direct
/// `self.stats.<counter>.fetch_add` patterns OUTSIDE the helper
/// bodies are forbidden — they let the global aggregate and the
/// per-request mirror diverge silently.
///
/// The matcher detects helper bodies via `fn record_*(` signatures
/// (walks forward to the next top-level `}\n`), then scans for the
/// counter names followed by `.fetch_add` within a 64-byte lookahead
/// (catches multi-line method-chain splits). Occurrences inside a
/// helper body are allowed; everything else is a violation.
///
/// The matcher logic lives in [`audit_counter_helper_violations`] so
/// the discriminator self-test below exercises the SAME code path.
/// Re-implementing the matcher in the self-test is a pinning gap:
/// loosening the production matcher (e.g. shrinking the 64-byte
/// lookahead) would silently weaken the guard while the self-test
/// passes against its independent matcher.
pub(super) fn audit_counter_helper_violations(src: &str) -> Vec<String> {
    // Identify the byte ranges that fall INSIDE one of the two
    // helper bodies. Helpers are short (one fetch_add each) and live
    // at top-level — match their `fn` signature, then walk forward
    // to the next top-level `}\n` (i.e. a `}` followed by a newline,
    // appearing at column 0).
    let helper_signatures = [
        "fn record_inflight_aborted_retry(",
        "fn record_cold_abort_swept(",
    ];
    let mut helper_ranges: Vec<(usize, usize)> = Vec::new();
    for sig in &helper_signatures {
        let mut search_start = 0usize;
        while let Some(rel_start) = src[search_start..].find(sig) {
            let abs_start = search_start + rel_start;
            // Find the closing `}` at column 0 after this start.
            let after = &src[abs_start..];
            let close_offset = after
                .find("\n}\n")
                .or_else(|| after.find("\n}\r\n"))
                .unwrap_or(after.len().saturating_sub(1));
            let abs_end = abs_start + close_offset + 2; // include the `}\n`
            helper_ranges.push((abs_start, abs_end));
            search_start = abs_end;
        }
    }

    // Scan for either of the two counter names followed by
    // `.fetch_add` within a 64-byte lookahead (covers multi-line
    // method-chain splits). Occurrences inside a helper body are
    // allowed; everything else is a violation reported with line
    // number + trimmed snippet.
    let counter_patterns = [".inflight_aborted_retries", ".cold_aborts_swept"];
    let mut violations: Vec<String> = Vec::new();
    for pattern in &counter_patterns {
        let mut search_start = 0usize;
        while let Some(rel) = src[search_start..].find(pattern) {
            let abs = search_start + rel;
            let lookahead_end = (abs + pattern.len() + 64).min(src.len());
            let lookahead = &src[abs..lookahead_end];
            let has_fetch_add = lookahead.contains(".fetch_add");
            search_start = abs + pattern.len();
            if !has_fetch_add {
                continue;
            }
            // Inside a helper body? Allow.
            let inside_helper = helper_ranges
                .iter()
                .any(|&(start, end)| abs >= start && abs < end);
            if inside_helper {
                continue;
            }
            // Compute line number for the report.
            let line_no = src[..abs].matches('\n').count() + 1;
            // Capture a small snippet around the violation.
            let snippet_start = src[..abs].rfind('\n').map(|i| i + 1).unwrap_or(0);
            let snippet_end = src[abs..].find('\n').map(|i| abs + i).unwrap_or(src.len());
            let snippet = &src[snippet_start..snippet_end];
            violations.push(format!("line {}: {}", line_no, snippet.trim()));
        }
    }
    violations
}

pub(super) fn walk_dir_collect_rs(dir: &std::path::Path, f: &mut dyn FnMut(&std::path::Path)) {
    let entries = std::fs::read_dir(dir).unwrap_or_else(|e| {
        panic!(
            "walk_dir_collect_rs: cannot read directory `{}`: {e}",
            dir.display()
        )
    });
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_dir_collect_rs(&path, f);
        } else if path.extension().is_some_and(|e| e == "rs") {
            f(&path);
        }
    }
}

/// AST scanner shared by [`host_upsert_performs_no_reverse_dependent_eviction`]
/// and its discriminating self-test. Flags any reverse-dependent or
/// own-canonical eager-drain method call: the bare identifiers
/// `reverse_deps_for` / `invalidate_canonical` / `evict_canonical`. A
/// bare `.clear()` is too generic to ban and is not flagged.
#[derive(Default)]
pub(super) struct UpsertEagerDrainScanner {
    pub(super) hits: Vec<String>,
}

impl UpsertEagerDrainScanner {
    /// Bare method identifiers that name an eager cache drain. None of
    /// these has a legitimate use inside `host_upsert.rs`.
    const FORBIDDEN_DRAIN_METHODS: &'static [&'static str] = &[
        "reverse_deps_for",
        "invalidate_canonical",
        "evict_canonical",
    ];
}

impl<'ast> syn::visit::Visit<'ast> for UpsertEagerDrainScanner {
    fn visit_expr_method_call(&mut self, mc: &'ast syn::ExprMethodCall) {
        let method = mc.method.to_string();
        if Self::FORBIDDEN_DRAIN_METHODS.contains(&method.as_str()) {
            self.hits.push(method.clone());
        }
        syn::visit::visit_expr_method_call(self, mc);
    }
}

/// Walk a directory and apply `cb` to every `.rs` or `.ts` file.
/// Shared by the protocol / FFI / compat scan above.
pub(super) fn walk_dir_collect_rs_and_ts(
    dir: &std::path::Path,
    cb: &mut dyn FnMut(&std::path::Path),
) {
    for entry in walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.path().is_file())
    {
        let path = entry.path();
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if matches!(ext, "rs" | "ts" | "tsx") {
            cb(path);
        }
    }
}

// ── Guard — compile_batch_options_has_no_thread_field ──
//
// Worker count for `compile_many` is fixed at host construction time
// (`HostConfig::host_cpu_threads`); the host-owned CPU pool is never
// resized per call. `CompileBatchOptions` must therefore carry NO
// per-call thread / concurrency knob. A per-call `threads` field
// (`CompileBatchOptions.threads`) was removed; re-adding any of
// `threads` / `thread_count` / `num_threads` would reintroduce a
// per-call concurrency surface and is a B7-scoped concept that does not
// belong on this options struct. (`CpuConcurrencySemaphore` and a
// per-call concurrency cap are the not-yet-built B7 design target.)
//
// Predicate: parse `crates/verter_session/src/host_compile.rs` via syn,
// find `pub struct CompileBatchOptions`, and assert none of its named
// fields is one of the banned thread-knob names.

pub(super) const BANNED_THREAD_FIELD_NAMES: &[&str] = &["threads", "thread_count", "num_threads"];

/// Pure core of the guard. Given parsed source and the target struct
/// name, returns `(found_struct, banned_fields_present)`. No I/O, so the
/// discriminator self-test can drive it against a synthetic struct.
pub(super) fn compile_batch_options_banned_thread_fields(
    parsed: &syn::File,
    target_struct: &str,
) -> (bool, Vec<String>) {
    use syn::{Fields, Item};
    let mut found_struct = false;
    let mut banned_present = Vec::<String>::new();
    for item in &parsed.items {
        let Item::Struct(s) = item else { continue };
        if s.ident != target_struct {
            continue;
        }
        found_struct = true;
        let Fields::Named(named) = &s.fields else {
            panic!(
                "{target_struct} is expected to have named fields; found {:?}",
                s.fields
            );
        };
        for field in &named.named {
            let field_name = field
                .ident
                .as_ref()
                .map(|id| id.to_string())
                .unwrap_or_default();
            if BANNED_THREAD_FIELD_NAMES.contains(&field_name.as_str()) {
                banned_present.push(field_name);
            }
        }
        break;
    }
    (found_struct, banned_present)
}

/// Return the byte span `[start, end)` of the body of `fn <fn_name>` in
/// `src` via brace matching, starting from the `{` that opens the body.
/// Panics if the function (or its opening brace) is not found — a moved
/// anchor must fail loudly rather than silently vacuously pass.
pub(super) fn fn_body_span(src: &str, fn_name: &str) -> (usize, usize) {
    let needle = format!("fn {fn_name}");
    let fn_at = src
        .find(&needle)
        .unwrap_or_else(|| panic!("guard anchor moved: `fn {fn_name}` not found"));
    let open = src[fn_at..]
        .find('{')
        .map(|o| fn_at + o)
        .unwrap_or_else(|| panic!("guard anchor moved: no `{{` after `fn {fn_name}`"));
    let bytes = src.as_bytes();
    let mut depth = 0usize;
    let mut i = open;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return (open, i + 1);
                }
            }
            _ => {}
        }
        i += 1;
    }
    panic!("guard anchor moved: unbalanced braces in `fn {fn_name}`");
}

// =====================================================================
// Non-current store-view contract — capability-split chokepoint guard.
//
// CRITICAL rule: "Non-current (`ReturnOnly`) store-view contract —
// capability split at the general accessor". The general store-view
// accessor MUST hand back the capability-split `StoreViewRead`, never a
// raw `HostStoreView`, so a warm validator cannot validate (or a
// query-returner cannot return) against a known-stale snapshot by
// accident. Warm-validation entry points accept ONLY a proven-current
// view (`&CurrentHostStoreView`); cold builders take a
// `ColdSeedHostStoreView`, which exposes NO `validates*` surface. The raw
// `HostStoreView` escape hatch (`StoreViewRead::into_owned_view`) is
// confined to an allowlist of request-driver / cold-seed / test-fixture
// producers that do not warm-validate against the value.
//
// These four parts are mechanically discriminating: each FAILS if the
// guarded invariant regresses (proven by the `_guard_is_discriminating`
// self-test below).
// =====================================================================

/// The single allowlist of production files permitted to unwrap a
/// `StoreViewRead` to a raw `HostStoreView` via `into_owned_view()`.
///
/// Every entry is a compile-fenced test fixture, a request-driver owned-view
/// snapshot accessor (currentness
/// gated separately by `snapshot_view_is_current`), a fenced cold-builder
/// seed (`.into_cold_seed_view().into_inner()`), or a `#[cfg(...)]`
/// test/debug fixture. NONE of them warm-validate a cache entry against
/// the unwrapped value. Adding a new production warm validator that grabs
/// a raw view fails [`resolver_store_view_into_owned_view_is_allowlisted`].
pub(super) const INTO_OWNED_VIEW_ALLOWLIST: &[&str] = &[
    // The capability-split producer and direct-host test fixtures. The host
    // context occurrence is confined to `with_bare_host_ctx_for_test` by its
    // `#[cfg(any(test, feature = "test-support"))]` fence; its production
    // lifecycle clones the already request-bound base view.
    "crates/verter_session/src/resolver_store.rs",
    "crates/verter_session/src/resolver_core/request_bound.rs",
    "crates/verter_session/src/resolver_core/host_resolver_context.rs",
    // Request-driver owned-view snapshot accessors (currentness gated by
    // `snapshot_view_is_current`, not by the unwrapped value).
    "crates/verter_session/src/host_manage.rs",
    "crates/verter_session/src/host_manage/component_meta_request_impl.rs",
    // Fenced cold-builder seeds (`.into_inner()`), gated by the driver's
    // `is_stable` / publish fence.
    "crates/verter_session/src/host_manage/component_meta_methods.rs",
    "crates/verter_session/src/host_manage/imported_type_root.rs",
    "crates/verter_session/src/host_resolve/frontier_engine.rs",
    // Build-time oracle-snapshot generator (`oracle-gen` feature only — never
    // on the consumption path): builds a quiescent owned view over a
    // freshly-constructed standalone host for the source-side walk.
    "crates/verter_session/src/typeinfo/oracle_core/gen.rs",
    // The shared, tsgo-free `source_admission_digest` derivation
    // (`#[cfg(any(test, feature = "oracle-gen"))]` only — never on the
    // production resolver path): builds the SAME quiescent owned view over a
    // freshly-constructed standalone host for the source-side walk, reached by
    // both the `oracle-gen` generator and the consumption guard
    // `source_admission_digest_consistent`.
    "crates/verter_session/src/typeinfo/oracle_core/source_digest.rs",
    // The v4 `relation_verdict` consumption driver (`#[cfg(test)]` only —
    // never on the production resolver path): builds the SAME quiescent owned
    // view over a freshly-constructed standalone host to observe the engine's
    // live `relate_nodes` answer for a registry relation spec, mirroring the
    // `support.rs` shallow-surface pattern. The raw-text scan cannot see the
    // cfg(test) boundary.
    "crates/verter_session/src/typeinfo/oracle_core/relation_driver.rs",
    // Inline `#[cfg(test)]` proof only (the input-side no-poison gate test
    // builds a quiescent owned view over a standalone host); no
    // production code path in this file touches the raw view — the raw-text
    // scan cannot see the cfg(test) boundary.
    "crates/verter_session/src/meta_resolve/projectors/output_sink.rs",
    // `#[cfg(any(test, feature = "test-support"))]` semantic-source probe
    // only (`demand_semantic_source_type_expr_with_ctx` builds an overlaid quiescent
    // view for session-published assertions); never compiled into the
    // production consumption path.
    "crates/verter_session/src/meta.rs",
];

/// `.rs` files of the session crate and of the type engine it builds on.
pub(super) fn store_view_guard_production_rs_files() -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    for krate in ["crates/verter_session/src", "crates/verter_type_engine/src"] {
        let root = workspace_root().join(krate);
        assert!(root.is_dir(), "source root {} is missing", root.display());
        walk_dir_collect_rs_and_ts(&root, &mut |path| {
            if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                files.push(path.to_path_buf());
            }
        });
    }
    files
}

pub(super) fn rel_path(path: &std::path::Path) -> String {
    path.strip_prefix(workspace_root())
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Whether a `src/**` file is a test/debug module (inline `#[cfg(test)]`
/// modules, `*_tests.rs`, `*/tests.rs`, `*/tests/*`, `typeinfo_tests`).
/// The contract is a PRODUCTION invariant; test/debug fixtures that build
/// a quiescent view for assertions are out of scope.
pub(super) fn store_view_guard_is_test_file(rel: &str) -> bool {
    rel.ends_with("_tests.rs")
        || rel.ends_with("/tests.rs")
        || rel.contains("/tests/")
        || rel.contains("/typeinfo_tests/")
}

/// The single allowlist of production files permitted to drop a cold-seed's
/// currentness via `ColdSeedHostStoreView::into_inner()`
/// (the `.into_cold_seed_view().into_inner()` raw-unwrap pattern).
///
/// Every entry is a NON-VALIDATING consumer of the unwrapped raw view:
///
/// * A request-driver `snapshot_store_view()` accessor — the driver gates
///   the snapshot's currentness SEPARATELY via `snapshot_view_is_current()`
///   and threads it into `compute(.., base_is_current)`; the raw view it
///   hands the driver is never the thing a nested validator reads through.
/// * A `#[cfg(any(test, debug_assertions))]` direct-`host` convenience
///   wrapper whose production counterpart routes through a ctx-bound
///   request boundary; under test the token never churns, so the seed is
///   always `Current` and `into_inner()` is harmless.
///
/// A NEW production cold-compute path that unwraps a cold-seed and feeds
/// the raw view into a resolver context performing NESTED warm-cache
/// validation MUST instead preserve the currentness — derive the cold-seed
/// from its own read via [`StoreViewRead::into_cold_seed_view`] (currentness
/// intrinsic to the arm), overlay-re-root via
/// [`ColdSeedHostStoreView::with_session_overlay`], then build the context
/// with `HostResolverContext::from_cold_seed` /
/// `SessionResolverContext::from_cold_seed`. An executor-snapshot path that
/// holds a single-read `(view, is_current)` pair re-binds it via
/// [`StoreViewRead::from_executor_snapshot`] — so a `ReturnOnly` seed fails
/// the context's `validates*` family closed. Adding such a path without
/// preserving currentness fails
/// [`cold_seed_into_inner_confined_to_non_validating_allowlist`].
pub(super) const COLD_SEED_INTO_INNER_ALLOWLIST: &[&str] = &[
    // The `ColdSeedHostStoreView::into_inner` definition + its sibling
    // `with_session_overlay` constructor.
    "crates/verter_session/src/resolver_store.rs",
    // Request-driver owned-view snapshot accessors (`snapshot_store_view`),
    // currentness gated by `snapshot_view_is_current` + threaded into
    // `compute(.., base_is_current)`, NOT by the unwrapped raw view.
    "crates/verter_session/src/host_manage.rs",
    "crates/verter_session/src/host_manage/component_meta_request_impl.rs",
    // The overlay-aware `capture_component_meta_inputs_with_view` accessor
    // unwraps a raw view ONLY to build `CapturedComponentMetaInputs` (source
    // + snapshot read) — a NON-validating consumer. The validating
    // cold-compute helpers in this file no longer unwrap: the view-bound and
    // overlay entries derive the cold-seed from a fresh read via
    // `into_cold_seed_view` (currentness intrinsic), and the
    // executor-snapshot `*_with_view_arg` entries re-bind the executor's
    // single-read pair via `from_executor_snapshot`.
    "crates/verter_session/src/host_manage/component_meta_methods.rs",
    // `#[cfg(any(test, debug_assertions))]` direct-`host` convenience
    // wrappers; production routes through a ctx-bound request boundary.
    "crates/verter_session/src/host_manage/imported_type_root.rs",
    "crates/verter_session/src/host_resolve/frontier_engine.rs",
    "crates/verter_session/src/host_resolve/route_surface.rs",
];

/// Whether `src` contains the cold-seed raw-unwrap escape-hatch pattern
/// `.into_cold_seed_view()` ... `.into_inner()` (tolerating intervening
/// whitespace / method-chain newlines).
pub(super) fn contains_cold_seed_into_inner(src: &str) -> bool {
    let mut search_from = 0;
    while let Some(rel) = src[search_from..].find(".into_cold_seed_view()") {
        let after = search_from + rel + ".into_cold_seed_view()".len();
        // The unwrap must be the NEXT method call in the chain (only
        // whitespace + the leading `.` between them); a `.is_current()` /
        // `.with_session_overlay(` / `.view()` in between means the
        // currentness was consulted, not dropped.
        let tail = src[after..].trim_start();
        if tail.starts_with(".into_inner()") {
            return true;
        }
        search_from = after;
    }
    false
}

/// Files permitted to call `StoreViewRead::from_executor_snapshot(view,
/// is_current)` — the one re-bind point that pairs a raw view with a
/// separately-named currentness bit.
///
/// Every entry is a stable-request EXECUTOR boundary where the `(view,
/// is_current)` pair provably came from a SINGLE
/// `resolver_store_view_with_currentness` read (the executor's
/// `snapshot_view` destructured one `StoreViewRead` and threaded both into
/// `compute`). A cold-compute helper that does its OWN fresh read must NOT
/// appear here — it must take the cold-seed straight from that read via
/// `into_cold_seed_view`, so the view and its currentness originate from one
/// read with no flag to mismatch.
pub(super) const FROM_EXECUTOR_SNAPSHOT_ALLOWLIST: &[&str] = &[
    // The constructor definition.
    "crates/verter_session/src/resolver_store.rs",
    // Fallthrough cold compute: re-binds the executor's `(store_view,
    // base_is_current)` pair (threaded from `snapshot_view`).
    "crates/verter_session/src/host_manage.rs",
    // Component-meta `*_with_view_arg` cold compute: re-binds the executor's
    // `(store_view, base_is_current)` pair. The view-bound + overlay entries
    // in this same file do NOT pair — they derive the cold-seed from a fresh
    // read via `into_cold_seed_view`; the guard below proves they take the
    // executor-supplied `store_view`, never a fresh `resolver_store_view_read`.
    "crates/verter_session/src/host_manage/component_meta_methods.rs",
    // `ViewBoundRequestHost::compute_component_meta` re-binds the executor's
    // `(store_view, base_is_current)` pair into the session-overlay cold-seed,
    // so the compute seed IS the read the promotion fence gates on. The
    // executor-supplied `store_view` parameter is re-bound (one executor read),
    // never a fresh `resolver_store_view_read()`; the `None` robustness arm
    // takes its cold-seed straight from a single fresh read via
    // `view_bound_cold_seed` (currentness intrinsic), so Rail 2 below stays
    // clean.
    "crates/verter_session/src/host_manage/component_meta_request_impl.rs",
];

/// Whether `src` contains the fresh-read-then-rebind footgun: a
/// `resolver_store_view_read()` whose result flows into
/// `StoreViewRead::from_executor_snapshot(` within the same statement chain.
///
/// This is the EXACT sub-class the constructor-shape guards missed — a fresh
/// second read paired with a currentness flag from an EARLIER read. The
/// production cold path must instead either (a) re-bind the EXECUTOR-supplied
/// `store_view` parameter (one executor read), or (b) take the cold-seed
/// straight from the fresh read via `into_cold_seed_view` (currentness
/// intrinsic). Pairing a fresh `resolver_store_view_read()` with
/// `from_executor_snapshot` mixes a fresh view with a foreign flag.
pub(super) fn contains_fresh_read_into_executor_snapshot(src: &str) -> bool {
    let mut search_from = 0;
    while let Some(rel) = src[search_from..].find("from_executor_snapshot(") {
        let abs = search_from + rel;
        // Look back over the immediately-preceding argument expression: if a
        // fresh `resolver_store_view_read()` feeds the first argument
        // (within the same `from_executor_snapshot( ... )` argument window),
        // the view came from a SECOND read while the flag is supplied
        // separately — the footgun.
        let arg_window_start = abs + "from_executor_snapshot(".len();
        // Bound the window at the matching close paren conservatively by the
        // next `.into_cold_seed_view()` or a 240-char cap (the call is a
        // single chained statement in production).
        let window_end = (arg_window_start + 240).min(src.len());
        let window = &src[arg_window_start..window_end];
        if window.contains("resolver_store_view_read()") {
            return true;
        }
        search_from = abs + "from_executor_snapshot(".len();
    }
    false
}

// Cold per-file artifact-build dedup guards.
//
// `ensure_indexed_ready_serve`'s materialise closure is the SINGLE per-file
// cold build: it parses once, builds one env, builds one shallow state,
// and publishes one `IndexedReady`. Declaration bodies live exclusively in
// the lazy `DeclBodyMemo`; no production path reparses a file the closure
// already parsed.
// ─────────────────────────────────────────────────────────────────────────

/// Byte mask over `body`: `true` for every byte inside a line comment,
/// block comment (nested), `"…"` string, raw/byte string, or char
/// literal. Used by [`strip_cfg_test_gated_source`] so a `#[cfg(test)]`
/// occurrence inside ANY comment or literal — including a line-leading
/// one inside a multi-line `/* … */` block — never starts a blanking
/// span.
pub(super) fn comment_and_string_mask(body: &str) -> Vec<bool> {
    let bytes = body.as_bytes();
    let mut mask = vec![false; bytes.len()];
    let mut i = 0usize;
    while i < bytes.len() {
        if body[i..].starts_with("//") {
            let end = body[i..].find('\n').map_or(bytes.len(), |o| i + o);
            for m in &mut mask[i..end] {
                *m = true;
            }
            i = end;
            continue;
        }
        if body[i..].starts_with("/*") {
            let mut depth = 1usize;
            let mut j = i + 2;
            while j < bytes.len() && depth > 0 {
                if body[j..].starts_with("/*") {
                    depth += 1;
                    j += 2;
                } else if body[j..].starts_with("*/") {
                    depth -= 1;
                    j += 2;
                } else {
                    j += 1;
                }
            }
            let end = j.min(bytes.len());
            for m in &mut mask[i..end] {
                *m = true;
            }
            i = end;
            continue;
        }
        if bytes[i] == b'"' {
            let mut j = i + 1;
            while j < bytes.len() {
                if bytes[j] == b'\\' {
                    j += 2;
                } else if bytes[j] == b'"' {
                    j += 1;
                    break;
                } else {
                    j += 1;
                }
            }
            let end = j.min(bytes.len());
            for m in &mut mask[i..end] {
                *m = true;
            }
            i = end;
            continue;
        }
        if bytes[i] == b'r' || bytes[i] == b'b' {
            // Possible raw/byte string start: `r"`, `r#"`, `b"`, `br#"`.
            let mut la = i + 1;
            if bytes[i] == b'b' && la < bytes.len() && bytes[la] == b'r' {
                la += 1;
            }
            let mut hashes = 0usize;
            while la < bytes.len() && bytes[la] == b'#' {
                hashes += 1;
                la += 1;
            }
            if la < bytes.len() && bytes[la] == b'"' {
                let closer = format!("\"{}", "#".repeat(hashes));
                let content_start = la + 1;
                let end = body[content_start..]
                    .find(&closer)
                    .map_or(bytes.len(), |o| content_start + o + closer.len());
                for m in &mut mask[i..end] {
                    *m = true;
                }
                i = end;
                continue;
            }
        }
        if bytes[i] == b'\'' {
            // Char literal vs lifetime — same heuristic as the extent
            // walk: a char literal closes within at most one escaped
            // char; a lifetime has no closing quote.
            let rest = &body[i + 1..];
            let mut it = rest.char_indices();
            if let Some((_, c1)) = it.next() {
                let close = if c1 == '\\' {
                    it.next();
                    it.next().map(|(_, c3)| c3) == Some('\'')
                } else {
                    it.next().map(|(_, c2)| c2) == Some('\'')
                };
                if close {
                    let end = i + 1 + rest.find('\'').map_or(0, |o| o + 1);
                    for m in &mut mask[i..end.min(bytes.len())] {
                        *m = true;
                    }
                    i = end;
                    continue;
                }
            }
        }
        i += 1;
    }
    mask
}

/// Blank every `#[cfg(test)]`-gated ITEM out of `body`, preserving
/// newlines so reported line numbers stay stable. The gated extent is
/// the attribute through the end of whatever it gates:
///
/// - the matching `}` of a brace-bodied item (fn / mod / impl),
/// - the terminating `;` of a statement-form item (`use`, a gated
///   statement),
/// - the terminating `,` of a STRUCT FIELD / enum variant / fn param
///   (fields end with `,`, not `;`/`}` — commas inside the field
///   type's generics / parens / brackets do not count),
/// - or, for a trailing field with no comma, the position JUST BEFORE
///   the enclosing delimiter's close (`}` / `)` / `]`), which is never
///   consumed — consuming it produced brace-unbalanced output that
///   failed `syn::parse_file` downstream and silently skipped whole
///   files in the route-mutator guard.
///
/// This is the scan-precision core of `session_production_ident_hits`:
/// the predecessor truncated each file at the FIRST `#[cfg(test)]`
/// occurrence, which left files whose first lines carry a test-only
/// `use` (e.g. `resolver_core/prepared_decl.rs`) almost entirely
/// unscanned. Only the exact `#[cfg(test)]` form is stripped —
/// `#[cfg(any(test, ...))]` items are conditionally compiled into
/// non-test builds, so they STAY scanned (strictly more coverage).
///
/// The item-extent walk is literal-aware: delimiters and terminators
/// inside line comments, block comments (nested), `"…"` strings,
/// `r#"…"#` raw strings, and char literals do not count. The marker
/// SEARCH is comment/string-masked too ([`comment_and_string_mask`]):
/// a `#[cfg(test)]` in doc-comment prose, a string literal, or a
/// multi-line block comment never starts a blanking span.
pub(super) fn strip_cfg_test_gated_source(body: &str) -> String {
    const MARKER: &str = "#[cfg(test)]";
    let bytes = body.as_bytes();
    let mask = comment_and_string_mask(body);
    let mut blanked: Vec<u8> = bytes.to_vec();
    let mut search_from = 0usize;
    while let Some(rel) = body[search_from..].find(MARKER) {
        let attr_start = search_from + rel;
        // A marker inside a comment or string literal is prose, not an
        // attribute — never a blanking span.
        if mask[attr_start] {
            search_from = attr_start + MARKER.len();
            continue;
        }
        // The marker must also be the FIRST non-whitespace on its line —
        // a genuine gate attribute is always line-leading
        // (rustfmt-enforced).
        let line_start = body[..attr_start].rfind('\n').map_or(0, |i| i + 1);
        if !body[line_start..attr_start].trim().is_empty() {
            search_from = attr_start + MARKER.len();
            continue;
        }
        let mut cursor = attr_start + MARKER.len();
        // Skip any further attributes between the marker and the item
        // header (`#[allow(...)]`, doc attrs, …).
        loop {
            let rest = &body[cursor..];
            let trimmed_len = rest.len() - rest.trim_start().len();
            let after_ws = cursor + trimmed_len;
            if body[after_ws..].starts_with("#[") {
                // Advance past this attribute's closing `]` (attributes
                // contain balanced brackets; track them literally).
                let mut depth = 0usize;
                let mut idx = after_ws;
                for (off, ch) in body[after_ws..].char_indices() {
                    match ch {
                        '[' => depth += 1,
                        ']' => {
                            depth -= 1;
                            if depth == 0 {
                                idx = after_ws + off + ch.len_utf8();
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                cursor = idx;
                continue;
            }
            cursor = after_ws;
            break;
        }
        // Walk to the end of the gated item: the first `;` at
        // brace/paren/bracket depth 0, the `}` that closes the first
        // opened brace block, a field/param-terminating `,` at
        // all-delimiter depth 0 (including generics' angle depth), or
        // — exclusively — the close of the ENCLOSING delimiter (a
        // trailing struct field / fn param).
        //
        // A `,` terminates ONLY a non-ITEM extent (a struct field, enum
        // variant, fn param, or gated statement/expression). A genuine
        // ITEM (fn / struct / use / …) never ends at a comma — but its
        // header can legitimately contain depth-0 commas (a `where` clause:
        // `fn f<F, R>(f: F) -> (usize, R) where F: FnOnce() -> R, {`),
        // so item extents keep walking to their `;` / body `}`. The
        // discriminator is the first keyword after the attributes
        // (with any `pub` / `pub(...)` visibility prefix skipped).
        let comma_terminates = {
            let head = body[cursor..].trim_start();
            let head = match head.strip_prefix("pub") {
                Some(rest) => {
                    let rest = rest.trim_start();
                    if let Some(inner) = rest.strip_prefix('(') {
                        match inner.find(')') {
                            Some(i) => inner[i + 1..].trim_start(),
                            None => rest,
                        }
                    } else {
                        rest
                    }
                }
                None => head,
            };
            const ITEM_KEYWORDS: &[&str] = &[
                "fn", "struct", "enum", "union", "trait", "impl", "mod", "use", "static", "const",
                "type", "unsafe", "extern", "async", "macro",
            ];
            let first_word: String = head
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            !ITEM_KEYWORDS.contains(&first_word.as_str())
        };
        let mut depth = 0usize;
        let mut paren_depth = 0usize;
        let mut bracket_depth = 0usize;
        let mut angle_depth = 0usize;
        // Previous significant char (delimiter-relevant, not consumed
        // by a literal/comment skip) — drives the `<` generic-open vs
        // less-than and `->` / `=>` arrow disambiguation.
        let mut prev_sig = '\0';
        let mut end = body.len();
        let mut chars = body[cursor..].char_indices().peekable();
        while let Some((off, ch)) = chars.next() {
            let abs = cursor + off;
            match ch {
                '/' => match chars.peek().map(|(_, c)| *c) {
                    Some('/') => {
                        // Line comment — consume to end of line.
                        for (o2, c2) in chars.by_ref() {
                            let _ = o2;
                            if c2 == '\n' {
                                break;
                            }
                        }
                    }
                    Some('*') => {
                        // Block comment — consume to matching `*/` (nested).
                        chars.next();
                        let mut bdepth = 1usize;
                        let mut prev = '\0';
                        for (_, c2) in chars.by_ref() {
                            if prev == '/' && c2 == '*' {
                                bdepth += 1;
                                prev = '\0';
                            } else if prev == '*' && c2 == '/' {
                                bdepth -= 1;
                                if bdepth == 0 {
                                    break;
                                }
                                prev = '\0';
                            } else {
                                prev = c2;
                            }
                        }
                    }
                    _ => {}
                },
                '"' => {
                    // String literal — consume with escapes.
                    let mut escaped = false;
                    for (_, c2) in chars.by_ref() {
                        if escaped {
                            escaped = false;
                        } else if c2 == '\\' {
                            escaped = true;
                        } else if c2 == '"' {
                            break;
                        }
                    }
                }
                'r' | 'b' => {
                    // Possible raw-string start: `r"`, `r#"`, `br#"`, `b"`.
                    let mut hashes = 0usize;
                    let mut la = abs + ch.len_utf8();
                    if ch == 'b' && body[la..].starts_with('r') {
                        la += 1;
                    }
                    while body[la..].starts_with('#') {
                        hashes += 1;
                        la += 1;
                    }
                    // A quote at the lookahead position means a raw/byte
                    // string literal starts here (valid Rust has no other
                    // `r…"` / `b…"` adjacency outside string content this
                    // scanner is already inside of).
                    if body[la..].starts_with('"') {
                        let closer = format!("\"{}", "#".repeat(hashes));
                        let body_after = la + 1;
                        let close_at = body[body_after..]
                            .find(&closer)
                            .map(|i| body_after + i + closer.len())
                            .unwrap_or(body.len());
                        while let Some((o2, _)) = chars.peek().copied() {
                            if cursor + o2 < close_at {
                                chars.next();
                            } else {
                                break;
                            }
                        }
                    }
                }
                '\'' => {
                    // Char literal vs lifetime: a char literal closes with
                    // `'` within at most one escaped char; a lifetime has
                    // no closing quote — leave it.
                    let rest = &body[abs + 1..];
                    let mut it = rest.char_indices();
                    if let Some((_, c1)) = it.next() {
                        let close = if c1 == '\\' {
                            it.next();
                            it.next().map(|(_, c3)| c3) == Some('\'')
                        } else {
                            it.next().map(|(_, c2)| c2) == Some('\'')
                        };
                        if close {
                            let consume_to = abs + 1 + rest.find('\'').map(|i| i + 1).unwrap_or(0);
                            while let Some((o2, _)) = chars.peek().copied() {
                                if cursor + o2 < consume_to {
                                    chars.next();
                                } else {
                                    break;
                                }
                            }
                        }
                    }
                }
                '{' => depth += 1,
                '}' => {
                    if depth == 0 {
                        // Close of the ENCLOSING block (a trailing
                        // struct field with no comma) — end the extent
                        // BEFORE it; consuming it leaves the output
                        // brace-unbalanced.
                        end = abs;
                        break;
                    }
                    depth -= 1;
                    // An item-body close terminates the extent ONLY at
                    // paren/bracket depth 0 — a closure block inside an
                    // argument list (`.with(|slot| { … });`) closes its
                    // brace while the statement continues to `);`.
                    if depth == 0 && paren_depth == 0 && bracket_depth == 0 {
                        end = abs + ch.len_utf8();
                        // A use-tree (`use x::{a, b};`) ends with `;`
                        // AFTER its brace group — consume it, or the
                        // stray top-level `;` fails syn downstream.
                        let rest = &body[end..];
                        let after_ws = end + (rest.len() - rest.trim_start().len());
                        if body[after_ws..].starts_with(';') {
                            end = after_ws + 1;
                        }
                        break;
                    }
                }
                '(' => paren_depth += 1,
                ')' => {
                    if paren_depth == 0 {
                        // Close of the enclosing paren list (a trailing
                        // gated fn param) — exclusive, same as `}`.
                        end = abs;
                        break;
                    }
                    paren_depth -= 1;
                }
                '[' => bracket_depth += 1,
                ']' => {
                    if bracket_depth == 0 {
                        end = abs;
                        break;
                    }
                    bracket_depth -= 1;
                }
                '<' => {
                    // Generic-open vs less-than: in item-header / field
                    // type position a `<` following an identifier char,
                    // `:` (paths), or another `<`/`>` opens generics.
                    if depth == 0
                        && paren_depth == 0
                        && bracket_depth == 0
                        && (prev_sig.is_alphanumeric()
                            || prev_sig == '_'
                            || prev_sig == ':'
                            || prev_sig == '<'
                            || prev_sig == '>')
                    {
                        angle_depth += 1;
                    }
                }
                '>' => {
                    // `->` / `=>` arrows are not generic closes.
                    if prev_sig != '-'
                        && prev_sig != '='
                        && depth == 0
                        && paren_depth == 0
                        && bracket_depth == 0
                    {
                        angle_depth = angle_depth.saturating_sub(1);
                    }
                }
                ';' if depth == 0 && paren_depth == 0 && bracket_depth == 0 => {
                    end = abs + ch.len_utf8();
                    break;
                }
                ',' if comma_terminates
                    && depth == 0
                    && paren_depth == 0
                    && bracket_depth == 0
                    && angle_depth == 0 =>
                {
                    // A struct-field / enum-variant / fn-param gate ends
                    // at its comma (fields end with `,`, not `;`/`}`).
                    end = abs + ch.len_utf8();
                    break;
                }
                _ => {}
            }
            if !ch.is_whitespace() {
                prev_sig = ch;
            }
        }
        // Doc comments and attributes immediately ABOVE the marker
        // attach to the gated item — leaving them behind produces a
        // dangling `///` with no following item, which is not a
        // parseable file (syn: "unexpected end of input"). Extend the
        // span upward over contiguous full-line `///` docs and `#[...]`
        // attributes.
        let mut span_start = line_start;
        while span_start > 0 {
            let prev_line_start = body[..span_start - 1].rfind('\n').map_or(0, |i| i + 1);
            let prev_line = body[prev_line_start..span_start - 1].trim();
            if prev_line.starts_with("///")
                || (prev_line.starts_with("#[") && prev_line.ends_with(']'))
            {
                span_start = prev_line_start;
            } else {
                break;
            }
        }
        // Blank the gated span, preserving newlines for stable line
        // numbers.
        for b in blanked[span_start..end].iter_mut() {
            if *b != b'\n' {
                *b = b' ';
            }
        }
        search_from = end.max(attr_start + MARKER.len());
    }
    String::from_utf8(blanked).expect("blanking preserves UTF-8 (ASCII spaces only)")
}

/// Per-body ident scan over PRODUCTION source: `#[cfg(test)]`-gated
/// items are stripped first; comment-only lines are skipped. Returns
/// 1-based line numbers of hits.
pub(super) fn ident_hits_in_production_body(
    body: &str,
    banned_idents: &[&str],
) -> Vec<(usize, String)> {
    let production_body = strip_cfg_test_gated_source(body);
    let mut hits = Vec::new();
    for ident in banned_idents {
        for (lineno, line) in production_body.lines().enumerate() {
            if !line.contains(ident) {
                continue;
            }
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") || trimmed.starts_with("///") || trimmed.starts_with("//!")
            {
                continue;
            }
            hits.push((lineno + 1, (*ident).to_string()));
        }
    }
    hits
}

/// Scan production `.rs` sources under `crates/verter_session/src` for a
/// banned identifier, skipping comment lines, file-level test sources
/// (`*_tests.rs` / `tests.rs`), and `#[cfg(test)]`-gated ITEMS (modules,
/// fns, uses — stripped by extent, NOT by truncating the file at the
/// first marker). An unreadable file is a hard failure — silent green on
/// I/O errors would make the guard decorative.
pub(super) fn session_production_ident_hits(banned_idents: &[&str]) -> Vec<(String, String)> {
    // The session crate and the type engine it builds on: both hold the
    // production code these bans protect.
    let crate_roots = [
        workspace_path("crates/verter_session/src"),
        workspace_path("crates/verter_type_engine/src"),
    ];
    for root in &crate_roots {
        assert!(root.is_dir(), "source root {} is missing", root.display());
    }
    let mut hits: Vec<(String, String)> = Vec::new();
    let mut scanned_files = 0usize;
    for entry in crate_roots
        .iter()
        .flat_map(|root| walkdir::WalkDir::new(root).into_iter())
        .filter_map(Result::ok)
        .filter(|e| e.path().is_file())
    {
        let path = entry.path();
        let path_str = path.to_string_lossy().replace('\\', "/");
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        if path_str.ends_with("_tests.rs")
            || path_str.ends_with("/tests.rs")
            || path_str.contains("/tests/")
            || path_str.contains("/typeinfo_tests/")
        {
            continue;
        }
        let body = std::fs::read_to_string(path)
            .unwrap_or_else(|err| panic!("guard scanner could not read {path_str}: {err}"));
        scanned_files += 1;
        for (lineno, ident) in ident_hits_in_production_body(&body, banned_idents) {
            hits.push((format!("{path_str}:{lineno}"), ident));
        }
    }
    assert!(
        scanned_files > 100,
        "guard scanner found only {scanned_files} production files under \
         crates/verter_session/src and crates/verter_type_engine/src — the \
         walk itself is broken",
    );
    hits
}

// ════════════════════════════════════════════════════════════════════════
// PARSELOWER carrier-contract foundation guards (additive).
//
// The TypeExpr→handle migration introduces session-owned hot carriers
// (`HotTypeRef`, the `BareRef` / `ImportType` / `RawFallback` graph carriers,
// the content-free `SyntheticBindingId`) plus the `CarrierResolverContext`
// value-side resolution bundle. These three guards pin the foundation
// invariants: the crate-ownership direction, the content-free synthetic
// identity, and the SCOPED ban on `Unknown`-as-control-flow inside the
// carrier surface.
// ════════════════════════════════════════════════════════════════════════

/// The brace-balanced body of `struct <name> { ... }` with line comments
/// stripped, so a field-token scan cannot be masked or falsely tripped by
/// DOC-comment prose.
pub(super) fn carrier_guard_struct_body(src: &str, name: &str) -> String {
    let needle = format!("struct {name} {{");
    let start = src
        .find(&needle)
        .unwrap_or_else(|| panic!("guard must find `struct {name}`"));
    let body_start = start + needle.len();
    let mut depth = 0usize;
    let mut end = body_start;
    for (offset, ch) in src[body_start..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                if depth == 0 {
                    end = body_start + offset;
                    break;
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    src[body_start..end]
        .lines()
        .map(|line| match line.find("//") {
            Some(i) => &line[..i],
            None => line,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The PRODUCTION portion of a Rust source: every line with `//` line-comments
/// AND `/* … */` block comments stripped, and every inline `#[cfg(test)]` ITEM
/// blanked IN PLACE — so production code that follows an inline cfg-test item
/// is still scanned (the weak split-once truncation lost it) and a forbidden
/// token mentioned inside a `/* */` block comment is never a false positive.
/// This matches the robust Stage-1 strippers. The carrier guards AND their
/// self-tests all scan through this helper, so the self-tests exercise the same
/// strip logic the guards rely on — never a bare `synthetic.contains(...)` that
/// would hold by construction.
pub(super) fn carrier_production_code(src: &str) -> String {
    carrier_strip_inline_cfg_test_items(&carrier_strip_comments(src))
}

/// Replace `//` line comments and `/* … */` (nesting) block comments with
/// equivalent-length whitespace (newlines preserved), skipping comment-like
/// sequences inside regular and raw string literals so the strip never
/// invalidates real source. Mirrors the robust Stage-1 `strip_comments`.
pub(super) fn carrier_strip_comments(src: &str) -> String {
    let bytes = src.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let n = bytes.len();
    let mut i = 0usize;
    while i < n {
        let c = bytes[i];
        // Raw string: r"..."  /  r#"..."#  /  r##"..."##  ...
        if c == b'r' {
            let mut j = i + 1;
            let mut hashes = 0usize;
            while j < n && bytes[j] == b'#' {
                hashes += 1;
                j += 1;
            }
            if j < n && bytes[j] == b'"' {
                out.extend_from_slice(&bytes[i..=j]);
                let close: Vec<u8> = std::iter::once(b'"')
                    .chain(std::iter::repeat_n(b'#', hashes))
                    .collect();
                let mut k = j + 1;
                while k + close.len() <= n {
                    if &bytes[k..k + close.len()] == close.as_slice() {
                        out.extend_from_slice(&bytes[(j + 1)..(k + close.len())]);
                        i = k + close.len();
                        break;
                    }
                    out.push(bytes[k]);
                    k += 1;
                }
                if k + close.len() > n {
                    out.extend_from_slice(&bytes[(j + 1)..n]);
                    i = n;
                }
                continue;
            }
            // Not a raw string — fall through to normal handling.
        }
        // Regular string literal "..." (with \" escape handling).
        if c == b'"' {
            out.push(b'"');
            let mut k = i + 1;
            while k < n {
                if bytes[k] == b'\\' && k + 1 < n {
                    out.push(bytes[k]);
                    out.push(bytes[k + 1]);
                    k += 2;
                    continue;
                }
                if bytes[k] == b'"' {
                    out.push(b'"');
                    k += 1;
                    break;
                }
                out.push(bytes[k]);
                k += 1;
            }
            i = k;
            continue;
        }
        // Char / byte-char literal 'x' / '\n' / '\u{…}' / '"' — disambiguated
        // from a lifetime (`'a` / `'static`, which has NO closing quote) like
        // rustc: a backslash escape, OR a single byte immediately followed by a
        // closing quote, is a char literal; anything else starting with `'`
        // falls through as a lifetime. This stops a `'"'` char literal from
        // mis-opening string mode and masking later source (the string arm
        // above only special-cases `"`).
        if c == b'\'' {
            // Escaped char literal `'\X…'`: scan to the unescaped closing quote.
            if i + 1 < n && bytes[i + 1] == b'\\' {
                out.push(b'\'');
                let mut k = i + 1;
                while k < n {
                    if bytes[k] == b'\\' && k + 1 < n {
                        out.push(bytes[k]);
                        out.push(bytes[k + 1]);
                        k += 2;
                        continue;
                    }
                    if bytes[k] == b'\'' {
                        out.push(b'\'');
                        k += 1;
                        break;
                    }
                    out.push(bytes[k]);
                    k += 1;
                }
                i = k;
                continue;
            }
            // Simple single-byte char literal `'x'` (close quote at i+2).
            if i + 2 < n && bytes[i + 2] == b'\'' {
                out.extend_from_slice(&bytes[i..=i + 2]);
                i += 3;
                continue;
            }
            // Otherwise a lifetime — fall through to normal byte handling.
        }
        // Line comment //
        if c == b'/' && i + 1 < n && bytes[i + 1] == b'/' {
            let mut k = i;
            while k < n && bytes[k] != b'\n' {
                out.push(b' ');
                k += 1;
            }
            i = k;
            continue;
        }
        // Block comment /* ... */ with nesting support.
        if c == b'/' && i + 1 < n && bytes[i + 1] == b'*' {
            let mut depth = 1u32;
            out.push(b' ');
            out.push(b' ');
            let mut k = i + 2;
            while k < n && depth > 0 {
                if k + 1 < n && bytes[k] == b'/' && bytes[k + 1] == b'*' {
                    depth += 1;
                    out.push(b' ');
                    out.push(b' ');
                    k += 2;
                    continue;
                }
                if k + 1 < n && bytes[k] == b'*' && bytes[k + 1] == b'/' {
                    depth -= 1;
                    out.push(b' ');
                    out.push(b' ');
                    k += 2;
                    continue;
                }
                if bytes[k] == b'\n' {
                    out.push(b'\n');
                } else {
                    out.push(b' ');
                }
                k += 1;
            }
            i = k;
            continue;
        }
        out.push(c);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Blank every `#[cfg(test)]`-attributed ITEM in place (newlines preserved)
/// instead of truncating at the first marker, so production code AFTER a
/// cfg-test item survives the scan. The blanked span runs from the attribute to
/// either the matching close brace of the item's first `{…}` body (an inline
/// `mod` / `fn` test item) or the `;` terminating a body-less declaration
/// (`#[cfg(test)] … mod foo;`), whichever comes first at item level —
/// string-aware so a `{` / `;` inside a string literal (e.g. a `#[path = "…"]`)
/// is skipped. Expects comment-stripped input (run after
/// [`carrier_strip_comments`]).
pub(super) fn carrier_strip_inline_cfg_test_items(src: &str) -> String {
    let bytes = src.as_bytes();
    let n = bytes.len();
    let mut out = bytes.to_vec();
    let needle = b"#[cfg(test)]";
    let mut i = 0usize;
    while i + needle.len() <= n {
        if &bytes[i..i + needle.len()] != needle {
            i += 1;
            continue;
        }
        // Find the end of this cfg-test item at item level.
        let mut j = i + needle.len();
        let mut end = n;
        let mut depth: i32 = 0;
        let mut started_body = false;
        while j < n {
            match bytes[j] {
                b'"' => {
                    // Skip a regular string literal (with \" escapes).
                    j += 1;
                    while j < n {
                        if bytes[j] == b'\\' && j + 1 < n {
                            j += 2;
                            continue;
                        }
                        if bytes[j] == b'"' {
                            j += 1;
                            break;
                        }
                        j += 1;
                    }
                    continue;
                }
                b'{' => {
                    depth += 1;
                    started_body = true;
                }
                b'}' => {
                    depth -= 1;
                    if started_body && depth == 0 {
                        end = j + 1;
                        break;
                    }
                }
                b';' if !started_body && depth == 0 => {
                    end = j + 1;
                    break;
                }
                _ => {}
            }
            j += 1;
        }
        for slot in out.iter_mut().take(end).skip(i) {
            if *slot != b'\n' {
                *slot = b' ';
            }
        }
        i = end;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Extract the union of EVERY comma-separated trait list across all STACKED
/// `#[derive(...)]` attributes in the contiguous attribute / `pub` / whitespace
/// / doc block immediately preceding `struct <name>` in `src`. Rust permits
/// multiple derive attributes to stack:
///
/// ```text
/// #[derive(Hash)]
/// #[derive(Debug, Clone, Copy)]
/// pub struct HotTypeRef(SemanticNodeId);
/// ```
///
/// A single `rfind("#[derive(")` would return only the LAST (Hash-free) line
/// and miss the `Hash` on the earlier stacked attribute — a silent R6 bypass.
/// This extractor walks BACKWARDS from the struct over the contiguous block,
/// collecting every stacked derive's trait list and stopping at the first
/// non-attribute item boundary (`;` / `}` / `struct ` / `enum ` / `fn ` / any
/// other code), so a far-away derive from an unrelated earlier struct never
/// leaks in (adjacency intent preserved). Panics if the struct or a preceding
/// derive is absent, or a derive is malformed — the guard fails LOUDLY rather
/// than passing vacuously. Both the real `HotTypeRef` guard and its self-test
/// call THIS extractor against the same shapes, so the self-test never
/// bypasses the real parsing logic.
pub(super) fn carrier_struct_derive_list(src: &str, name: &str) -> String {
    let needle = format!("struct {name}");
    let struct_pos = src
        .find(&needle)
        .unwrap_or_else(|| panic!("guard must find `struct {name}`"));
    // Walk the lines preceding `struct <name>` in reverse. The final prefix
    // line is the struct's own line content up to (not including) the `struct`
    // keyword — e.g. the `pub ` in `pub struct HotTypeRef(...)`. Only
    // attributes / `pub` / blank / doc-comment lines may sit in the contiguous
    // block; the first line that is none of those is the item boundary.
    let prefix = &src[..struct_pos];
    let mut lists: Vec<String> = Vec::new();
    for raw_line in prefix.lines().rev() {
        let line = raw_line.trim();
        // Contiguous-block filler that may legitimately sit between stacked
        // derives and the struct: blank lines, doc / line comments, or a
        // `pub` / `pub(crate)` visibility token on the struct's own line.
        // Checked FIRST so a doc comment that merely MENTIONS `#[derive(Hash)]`
        // in prose (the real `HotTypeRef` rustdoc does exactly this) is never
        // mistaken for an actual derive attribute.
        if line.is_empty()
            || line.starts_with("///")
            || line.starts_with("//!")
            || line.starts_with("//")
            || line == "pub"
            || line == "pub(crate)"
        {
            continue;
        }
        // A REAL derive attribute, trimmed, STARTS WITH `#[derive(` — match by
        // prefix (not substring) so only a genuine attribute contributes.
        if let Some(rest) = line.strip_prefix("#[derive(") {
            let close = rest
                .find(')')
                .unwrap_or_else(|| panic!("malformed `#[derive(...)]` before `struct {name}`"));
            lists.push(rest[..close].to_string());
            continue;
        }
        // A non-derive attribute (`#[repr(C)]`, `#[cfg(...)]`) is still part of
        // the contiguous attribute block; keep walking.
        if line.starts_with("#[") || line.starts_with("#![") {
            continue;
        }
        // Anything else is a non-attribute item boundary (`;` / `}` /
        // `struct ` / `enum ` / `fn ` / a prior decl): the contiguous block
        // ends here, so an unrelated earlier struct's derive cannot leak in.
        break;
    }
    assert!(
        !lists.is_empty(),
        "guard must find a `#[derive(...)]` preceding `struct {name}`"
    );
    // Source-order (top-down) union, comma-joined: the predicate splits on `,`
    // so duplicates are harmless and the failure message reads naturally.
    lists.reverse();
    lists.join(", ")
}

/// True iff a derive trait list contains `Hash` or `Ord` as a WHOLE trait
/// token (split on `,`, trimmed). Whole-token matching is the discriminating
/// detail: `PartialOrd` / `PartialEq` must NOT register as a substring
/// false-positive for `Ord`. A handle that derived either trait could be
/// lifted into a `HashMap` / `BTreeMap` cache key, breaking R6.
pub(super) fn derive_list_has_hash_or_ord(list: &str) -> bool {
    list.split(',')
        .map(str::trim)
        .any(|t| t == "Hash" || t == "Ord")
}

/// Extract the field list of an `enum` variant declared as
/// `Variant { field: Ty, ... }` from `src`. Returns the text between the
/// variant's `{` and its matching `}` with `//` line comments stripped.
/// Used to assert a struct-like enum variant's field list is content-free.
pub(super) fn enum_variant_struct_body(src: &str, variant: &str) -> String {
    let needle = format!("{variant} {{");
    let start = src
        .find(&needle)
        .unwrap_or_else(|| panic!("guard must find enum variant `{variant} {{`"));
    let body_start = start + needle.len();
    let mut depth = 0usize;
    let mut end = body_start;
    for (offset, ch) in src[body_start..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                if depth == 0 {
                    end = body_start + offset;
                    break;
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    src[body_start..end]
        .lines()
        .map(|line| match line.find("//") {
            Some(i) => &line[..i],
            None => line,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Whether a NAMED fn's PARAMETER types mention `type_name` — SIGNATURE-SCOPED,
/// not a whole-file substring. Parses `src`, visits every free fn / impl method
/// named `fn_name`, and returns true iff ANY of THAT fn's parameter type token
/// streams contains `type_name` as an ident. (A whole-file
/// `src.contains("AdmittedPublishedMember")` would hold because the token name
/// appears in many places; this restricts the check to the fn's own params, so
/// a by-value `from_surface_member(member: SemanticNodeId)` does NOT pass merely
/// because the token name appears elsewhere in the file.)
pub(super) fn named_fn_param_mentions_type(src: &str, fn_name: &str, type_name: &str) -> bool {
    use quote::ToTokens;
    struct V<'a> {
        fn_name: &'a str,
        type_name: &'a str,
        found: bool,
    }
    impl<'a> V<'a> {
        fn check(&mut self, sig: &syn::Signature) {
            if sig.ident != self.fn_name {
                return;
            }
            for input in &sig.inputs {
                if let syn::FnArg::Typed(pat) = input {
                    let mentions = pat.ty.to_token_stream().into_iter().any(|tt| match tt {
                        proc_macro2::TokenTree::Ident(id) => id == self.type_name,
                        _ => false,
                    }) || pat
                        .ty
                        .to_token_stream()
                        .to_string()
                        .contains(self.type_name);
                    if mentions {
                        self.found = true;
                    }
                }
            }
        }
    }
    impl<'ast> syn::visit::Visit<'ast> for V<'_> {
        fn visit_item_fn(&mut self, f: &'ast syn::ItemFn) {
            self.check(&f.sig);
            syn::visit::visit_item_fn(self, f);
        }
        fn visit_impl_item_fn(&mut self, f: &'ast syn::ImplItemFn) {
            self.check(&f.sig);
            syn::visit::visit_impl_item_fn(self, f);
        }
    }
    let Ok(file) = syn::parse_file(src) else {
        return false;
    };
    let mut v = V {
        fn_name,
        type_name,
        found: false,
    };
    syn::visit::Visit::visit_file(&mut v, &file);
    v.found
}

/// Recursively collect production (`*.rs`, excluding `*_tests.rs`) files under
/// `dir`. Test files are excluded — the guard locks the PRODUCTION worker
/// surface; test code legitimately references session-graph types.
pub(super) fn collect_production_rs(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
    if !dir.exists() {
        return;
    }
    for entry in fs::read_dir(dir).unwrap_or_else(|e| panic!("read {}: {e}", dir.display())) {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            collect_production_rs(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs")
            && !path
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.ends_with("_tests.rs"))
                .unwrap_or(false)
        {
            out.push(path);
        }
    }
}

/// The structural lowerer's defining module: the raw entry
/// the THREE producer-capable builders (`lower_type_expr_structural`,
/// `build_macro_hot_ref`, `build_script_setup_seed_frames`) each live under
/// `crate::structural_carrier_producer::macro_arg_producer` and are EACH
/// single-defined + MODULE-PRIVATE (no visibility modifier). The module itself
/// is a PRIVATE `mod macro_arg_producer;` (only `macro_type_arg_hot_ref` +
/// `MacroHotMirror` are re-exported), so no FOREIGN module can NAME any builder
/// — a second producer in a foreign file is a compile error (E0603 / E0433),
/// UNREPRESENTABLE by construction. The SAME-MODULE case is different: Rust
/// privacy is module-scoped, so a SECOND producer written INSIDE this file CAN
/// name the module-private builders; the owner's collapse to one file does NOT
/// make that a compile error. This privacy-shape assertion is therefore one
/// of the BOUNDED single-producer guards that POLICE that same-module residual
/// (alongside the producer-exposure / no-codegen-surface / no-query /
/// derive-shadow guards) — it pins each builder bare-private + single-defined +
/// not re-exported, so a widened or duplicated builder reddens.
pub(super) const STRUCTURAL_CARRIER_PRODUCER_LOWERER_MODULE: &str =
    "structural_carrier_producer/macro_arg_producer.rs";

/// The THREE producer-capable builders that MUST each be a single-defined, bare
/// module-private `fn` (no visibility modifier) inside `macro_arg_producer.rs`
/// and re-exported NOWHERE: the raw query-free structural lowerer, the macro
/// hot-mirror builder, and the `<script setup generic="…">` binder-seed builder.
/// The ONLY crate-visible producer entry is `macro_type_arg_hot_ref` (asserted by
/// the entry-surface guard, NOT this one). A `pub` / `pub(crate)` / `pub(in …)` on
/// ANY of these three, or a second definition, or a `pub use` re-export of any of
/// them, re-opens a second/third structural-carrier producer surface.
pub(super) const STRUCTURAL_CARRIER_PRODUCER_PRIVATE_BUILDERS: &[&str] = &[
    "lower_type_expr_structural",
    "build_macro_hot_ref",
    "build_script_setup_seed_frames",
];

/// Classify the visibility shape of the named producer builder's definition in
/// the given source body. Returns the offending reason when the entry carries
/// ANY visibility modifier (the required shape is a bare module-private
/// `fn <builder>(` — no `pub`, no `pub(crate)`, no `pub(in …)`), or `None` when
/// the shape is correct OR the builder is absent from `body` (absence is handled
/// by the caller's anti-vacuity + single-definition assertions, NOT here). Looks
/// at the DEFINITION line only.
pub(super) fn structural_carrier_producer_builder_privacy_violation(
    body: &str,
    builder: &str,
) -> Option<String> {
    let needle = format!("fn {builder}(");
    let def_line = body.lines().find(|l| l.contains(&needle))?;
    let trimmed = def_line.trim_start();
    // The ONLY accepted shape is the bare module-private `fn` — the definition
    // line starts directly with `fn <builder>(`. ANY visibility modifier (`pub`,
    // `pub(crate)`, `pub(super)`, `pub(in …)`) lets some other module name the
    // entry and is the forbidden second/third-producer shape.
    if trimmed.starts_with(&needle) {
        return None;
    }
    Some(format!(
        "the producer builder `{builder}`'s entry must be a bare module-private \
         `fn {builder}(` (NO visibility modifier at all — not `pub`, `pub(crate)`, or \
         `pub(in …)`), so no other module can name it and only the single sanctioned producer \
         entry can reach it; found: `{trimmed}`"
    ))
}

/// Whether `body` re-exports the named producer builder with a
/// `pub use … <builder>` / `pub use … as <builder>` binding — a re-export would
/// mint a NAMEABLE alias of the otherwise-private builder, re-opening the producer
/// surface. The accepted module-private fn cannot be `pub use`-re-exported at all
/// (that is itself a compile error), so this scan finds nothing on the genuine
/// tree; it RED-proves a re-export evasion.
pub(super) fn structural_builder_reexport_violation(body: &str, builder: &str) -> bool {
    body.lines().any(|line| {
        let trimmed = line.trim_start();
        trimmed.starts_with("pub use ")
            && trimmed
                .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .any(|token| token == builder)
    })
}

/// Whether `attr` is EXACTLY `#[cfg(test)]` — the meta is a single-token `cfg`
/// list whose sole token is the bare identifier `test`. This is STRICT by
/// design: it REJECTS `#[cfg(any(test, …))]`, `#[cfg(all(test, …))]`,
/// `#[cfg(feature = "…")]`, `#[cfg(not(test))]`, `#[cfg_attr(…)]`, and any other
/// cfg that is SATISFIABLE in a non-test production build — only `cfg(test)`,
/// which is unsatisfiable in a normal build, is a true test-only gate. (The
/// prior `any token == "test"` matcher accepted `cfg(any(test, feature = "x"))`,
/// which compiles in production under feature `x` — the hole this closes.)
pub(super) fn attr_is_exactly_cfg_test(attr: &syn::Attribute) -> bool {
    if !attr.path().is_ident("cfg") {
        return false;
    }
    match &attr.meta {
        syn::Meta::List(list) => {
            // The token stream must be the single bare ident `test` — no nested
            // `any(…)` / `all(…)` / `feature = …` / `not(…)` / commas / parens.
            let mut tokens = list.tokens.clone().into_iter();
            match (tokens.next(), tokens.next()) {
                (Some(proc_macro2::TokenTree::Ident(id)), None) => id == "test",
                _ => false,
            }
        }
        // `#[cfg]` / `#[cfg = "…"]` are not a `cfg(test)` gate.
        _ => false,
    }
}

/// Whether an out-of-line `mod` `m` is a SANCTIONED test-wiring child of the
/// producer module: `#[cfg(test)] #[path = "<name>.rs"] mod <name>;` where the
/// module name ends with `_tests` and the `#[path]` is EXACTLY the sibling
/// `<name>.rs`. The producer module wires three such test children
/// (`structural_lower_tests`, `macro_hot_mirror_tests`,
/// `script_setup_binder_tests`). ALL of the following must hold, exactly —
/// anything else is a production out-of-line child module (rejected):
///
/// - the module name ends with `_tests` (the unit-test sibling convention);
/// - it carries EXACTLY one `#[cfg(test)]` ([`attr_is_exactly_cfg_test`]) — not
///   `cfg(any(test, …))` / `cfg(all(…))` / a feature cfg / `cfg_attr`, so it is
///   never compiled into a production build;
/// - it carries EXACTLY one `#[path = "<name>.rs"]` matching its own module name
///   — not a `../`-escaping or any other path (the test body is the named
///   SIBLING file, not a foreign file pulled in under a test gate);
/// - it carries NO OTHER attributes (the attribute set is precisely
///   `{cfg(test), path}` — a smuggled extra attribute, e.g. a rewriting
///   proc-macro attribute, is rejected).
pub(super) fn mod_is_sanctioned_test_wiring(m: &syn::ItemMod) -> bool {
    let mod_name = m.ident.to_string();
    if !mod_name.ends_with("_tests") {
        return false;
    }
    let expected_path = format!("{mod_name}.rs");
    let mut saw_cfg_test = false;
    let mut saw_exact_path = false;
    for attr in &m.attrs {
        if attr_is_exactly_cfg_test(attr) {
            // Reject a SECOND cfg attribute (only one `cfg(test)` is sanctioned).
            if saw_cfg_test {
                return false;
            }
            saw_cfg_test = true;
            continue;
        }
        if attr.path().is_ident("path") {
            if saw_exact_path {
                return false;
            }
            // The path value must be EXACTLY the sibling `<name>.rs`.
            let is_exact = match &attr.meta {
                syn::Meta::NameValue(nv) => match &nv.value {
                    syn::Expr::Lit(syn::ExprLit {
                        lit: syn::Lit::Str(s),
                        ..
                    }) => s.value() == expected_path,
                    _ => false,
                },
                _ => false,
            };
            if !is_exact {
                return false;
            }
            saw_exact_path = true;
            continue;
        }
        // Any OTHER attribute (a non-`cfg(test)` cfg, a `cfg_attr`, a derive /
        // attribute proc-macro, …) disqualifies the sanctioned wiring.
        return false;
    }
    saw_cfg_test && saw_exact_path
}

/// The SINGLE-SEGMENT, compiler-built-in DERIVE names the producer module's data
/// types legitimately carry (`#[derive(Debug, Default, Clone, Copy, PartialEq,
/// Eq)]` on `BinderScope` / `StructuralLowerContext` / `StructuralLowerError` /
/// `MacroHotMirror`). A built-in derive is a compiler-known, fully-determined
/// expansion — it cannot synthesise a call to the module-private builders from
/// elsewhere. A derive whose path is NOT a single segment (`evil::Debug`) or
/// whose name is not one of these built-ins is rejected (FIX E — derives are 6a
/// SCOPED, not banned). The derive-SHADOW vector — an import that brings one of
/// THESE built-in names into scope under a foreign definition — is rejected
/// separately (see [`macro_arg_producer_derive_shadow_import_violations`]).
pub(super) const MACRO_ARG_PRODUCER_BUILTIN_DERIVES: &[&str] = &[
    "Debug",
    "Default",
    "Clone",
    "Copy",
    "PartialEq",
    "Eq",
    "Hash",
    "PartialOrd",
    "Ord",
];

/// Collect PRODUCTION expansion-surface violations in `macro_arg_producer.rs`
/// source `src` — non-`#[cfg(test)]` constructs that re-introduce a same-module
/// code-generation surface able to emit code reaching the module-private
/// lowering builders without naming them literally. The single-producer
/// guarantee is the COMPILER module-privacy of `macro_arg_producer.rs`; this is
/// the small no-reintroduce-a-surface backstop the structural design's residual
/// names, NOT a load-bearing scanner. Rejected (each a `syn`-visible production
/// surface):
///
/// - ANY production bang-macro INVOCATION (item / expr / stmt position) — the
///   `visit_macro` override rejects every `syn::Macro` invocation except the
///   item-position `macro_rules!` (itself rejected through the item path below).
///   The ban is ALL, NOT a name-denylist: a function-like macro DEFINED anywhere
///   (mod.rs / a foreign crate / a proc-macro crate) and invoked here is invisible
///   to `syn` (it never expands it), so a denylist of specific names (`include!`,
///   `concat_idents!`, `paste!`, …) is inherently incomplete. The real file is
///   therefore kept bang-macro-FREE: the former `matches!` was de-sugared to a
///   `match` and the former `vec![…]` to `Vec::from(…)`, so the ban-all rule
///   passes on the genuine tree;
/// - a CUSTOM (non-builtin) `#[derive(…)]` — a derive proc-macro expands in the
///   module context (a built-in derive on the producer's data types is allowed);
/// - a `#[macro_use]` attribute (the prelude-injection vector) or any non-inert
///   attribute proc-macro on a producer-capable item;
/// - an out-of-line / `#[path]` child `mod` that is NOT the sanctioned
///   `#[cfg(test)] #[path] mod *_tests;` test wiring.
///
/// Returns the list of human-readable violation strings (empty on the genuine
/// tree). PRODUCTION-only: a `#[cfg(test)]`-gated item (the unit test children)
/// is skipped — the `visit_item` override returns early on a `cfg(test)` item,
/// so neither its macros nor its child items are visited.
pub(super) fn macro_arg_producer_expansion_surface_violations(src: &str) -> Vec<String> {
    use syn::visit::Visit;
    let file = match syn::parse_file(src) {
        Ok(f) => f,
        Err(e) => return vec![format!("parse error: {e}")],
    };
    let mut visitor = ExpansionSurfaceVisitor {
        violations: Vec::new(),
    };
    visitor.visit_file(&file);
    visitor.violations
}

/// `syn::Visit` over a production `macro_arg_producer.rs` parse: classifies every
/// macro position, every producer-capable attribute (custom `#[derive]` /
/// `#[macro_use]`), and every out-of-line child mod — skipping `#[cfg(test)]`-gated
/// items by overriding `visit_item` to return early on a `cfg(test)` item (so no
/// test-only macro / mod / derive is ever counted) and an item-position
/// `macro_rules!` definition.
pub(super) struct ExpansionSurfaceVisitor {
    pub(super) violations: Vec<String>,
}

impl ExpansionSurfaceVisitor {
    /// Check an item's attribute list: a `#[macro_use]` (prelude injection), a
    /// CUSTOM (non-builtin) `#[derive(…)]`, or any qualified / proc-macro
    /// attribute is a code-generation surface. `#[cfg]` / `#[allow]` / `#[expect]`
    /// / `#[path]` / `#[doc]` and a built-in `#[derive(…)]` are inert and allowed.
    fn check_attrs(&mut self, attrs: &[syn::Attribute]) {
        for attr in attrs {
            let path = attr.path();
            let single = path.segments.len() == 1;
            let last = path
                .segments
                .last()
                .map(|s| s.ident.to_string())
                .unwrap_or_default();
            if single && last == "macro_use" {
                self.violations
                    .push("a production `#[macro_use]` attribute is forbidden".to_string());
                continue;
            }
            if single && last == "derive" {
                // FIX E (derives = 6a SCOPED): each listed derive path must be a
                // SINGLE-SEGMENT, compiler-built-in name. A QUALIFIED path
                // (`evil::Debug`) is rejected even though its final segment is a
                // built-in name — a qualified derive resolves to a FOREIGN macro,
                // not the compiler built-in. A non-built-in single-segment name is
                // a custom proc-macro derive and is rejected.
                let _ = attr.parse_nested_meta(|meta| {
                    let derive_path = &meta.path;
                    let segs = derive_path.segments.len();
                    let name = derive_path
                        .segments
                        .last()
                        .map(|s| s.ident.to_string())
                        .unwrap_or_default();
                    if segs != 1 {
                        self.violations.push(format!(
                            "a production QUALIFIED `#[derive({})]` is forbidden — a derive path \
                             must be a SINGLE-SEGMENT compiler built-in; a qualified path resolves \
                             to a foreign derive macro that can synthesise same-module code",
                            derive_path
                                .segments
                                .iter()
                                .map(|s| s.ident.to_string())
                                .collect::<Vec<_>>()
                                .join("::")
                        ));
                    } else if !MACRO_ARG_PRODUCER_BUILTIN_DERIVES.contains(&name.as_str()) {
                        self.violations.push(format!(
                            "a production custom `#[derive({name})]` (a derive proc-macro) is \
                             forbidden — only std built-in derives are allowed"
                        ));
                    }
                    Ok(())
                });
                continue;
            }
            // Inert attributes that cannot rewrite / synthesise an item.
            if single && matches!(last.as_str(), "cfg" | "allow" | "expect" | "path" | "doc") {
                continue;
            }
            // Everything else — a qualified (multi-segment) attribute path or any
            // other single-segment attribute (an attribute proc-macro) — is a
            // code-generation surface.
            self.violations.push(format!(
                "a production attribute `#[{}]` is forbidden — only `cfg` / `allow` / `expect` / \
                 `path` / `doc` / a built-in `derive(…)` are inert; a qualified or proc-macro \
                 attribute can rewrite a producer item to synthesise same-module code",
                path.segments
                    .iter()
                    .map(|s| s.ident.to_string())
                    .collect::<Vec<_>>()
                    .join("::")
            ));
        }
    }
}

impl<'ast> syn::visit::Visit<'ast> for ExpansionSurfaceVisitor {
    fn visit_item(&mut self, item: &'ast syn::Item) {
        // Skip `#[cfg(test)]`-gated items entirely (they never compile into a
        // production build); do NOT descend, so their macros / child mods are
        // never counted.
        let attrs: &[syn::Attribute] = match item {
            syn::Item::Mod(m) => &m.attrs,
            syn::Item::Macro(m) => &m.attrs,
            syn::Item::Fn(f) => &f.attrs,
            syn::Item::Struct(s) => &s.attrs,
            syn::Item::Enum(e) => &e.attrs,
            syn::Item::Impl(i) => &i.attrs,
            syn::Item::Use(u) => &u.attrs,
            syn::Item::Const(c) => &c.attrs,
            syn::Item::Static(s) => &s.attrs,
            syn::Item::Type(t) => &t.attrs,
            syn::Item::Trait(t) => &t.attrs,
            syn::Item::ExternCrate(e) => &e.attrs,
            syn::Item::TraitAlias(t) => &t.attrs,
            syn::Item::Union(u) => &u.attrs,
            syn::Item::ForeignMod(f) => &f.attrs,
            _ => &[],
        };
        if attrs.iter().any(attr_is_exactly_cfg_test) {
            return;
        }
        self.check_attrs(attrs);
        match item {
            // An item-position `macro_rules!` DEFINITION is a code-generation
            // surface (`syn::ItemMacro` whose path is `macro_rules`).
            syn::Item::Macro(m) if m.mac.path.is_ident("macro_rules") => {
                self.violations
                    .push("a production `macro_rules!` definition is forbidden".to_string());
            }
            // An out-of-line / `#[path]` child mod that is NOT the sanctioned
            // `#[cfg(test)] #[path] mod *_tests;` wiring is a production splice
            // surface.
            syn::Item::Mod(m) if m.content.is_none() && !mod_is_sanctioned_test_wiring(m) => {
                self.violations.push(format!(
                    "a production out-of-line / `#[path]` child mod `{}` is forbidden (only the \
                     sanctioned `#[cfg(test)] #[path] mod *_tests;` wiring is allowed)",
                    m.ident
                ));
            }
            _ => {}
        }
        // Recurse into the (non-cfg-test) item so nested items, fn bodies, and
        // their macro positions are visited.
        syn::visit::visit_item(self, item);
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        // FIX C — ban ALL production bang-macro INVOCATIONS (item / expr / stmt
        // position). A function-like macro DEFINED ANYWHERE (mod.rs / a foreign
        // crate / a proc-macro crate) and INVOKED here is invisible to `syn` (it
        // never expands it), so the generated producer is unseen — a denylist of
        // specific macro names is inherently incomplete. The `matches!`→`match`
        // de-sugar in production leaves the real file bang-macro-free, so the
        // ban-all rule passes on the genuine tree. `macro_rules!` DEFINITIONS are
        // handled at item position in `visit_item` (skipped here to avoid a
        // double report).
        let name = mac
            .path
            .segments
            .last()
            .map(|s| s.ident.to_string())
            .unwrap_or_default();
        if name != "macro_rules" {
            self.violations.push(format!(
                "a production bang-macro invocation `{name}!(…)` is forbidden in the producer \
                 module — a macro can expand into code reaching the module-private lowering \
                 builders without naming them; the producer module is kept bang-macro-free (the \
                 one `matches!` is de-sugared to a `match`)"
            ));
        }
        syn::visit::visit_macro(self, mac);
    }
}

/// Unraw an identifier string: `r#Clone` → `Clone`. A raw-ident import
/// (`use evil::r#Clone;`) brings the SAME name `Clone` into scope as a plain
/// `use evil::Clone;`, so the derive-shadow check must compare against the unraw
/// form.
pub(super) fn unraw_ident(s: &str) -> &str {
    s.strip_prefix("r#").unwrap_or(s)
}

/// The NAME a `use`-tree LEAF binds into the local namespace — the alias for a
/// `Rename` (`X as Clone` binds `Clone`), the leaf ident for a `Name`, unrawed.
/// `None` for a glob (handled separately) or a non-leaf.
pub(super) fn use_leaf_bound_name(tree: &syn::UseTree) -> Option<String> {
    match tree {
        syn::UseTree::Name(n) => Some(unraw_ident(&n.ident.to_string()).to_string()),
        syn::UseTree::Rename(r) => Some(unraw_ident(&r.rename.to_string()).to_string()),
        _ => None,
    }
}

/// FIX E (derive-SHADOW import rejection, scoped to `macro_arg_producer.rs`):
/// collect production `use` / `pub use` imports (and `#[macro_use]` attributes)
/// that could bring one of the built-in-derive names
/// ([`MACRO_ARG_PRODUCER_BUILTIN_DERIVES`]) into scope under a FOREIGN definition,
/// shadowing the compiler built-in so a `#[derive(Clone)]` resolves to the
/// foreign macro. Rejected:
/// - a `use …::Clone;` / `use …::r#Clone;` whose bound name is a built-in-derive
///   name;
/// - a `use …::Foo as Clone;` alias whose ALIAS is a built-in-derive name;
/// - a GLOB `use foo::*;` (it could bring ANY of the built-in names into scope —
///   undecidable from the `use` alone, so conservatively rejected);
/// - a `#[macro_use]` attribute (prelude injection of derive macros).
///
/// It ALSO bans rebinding the crate-root names `std`/`core` in the module — a `use
/// … as std;` / `use … as core;` alias (or a single-segment `use std;`/`use
/// core;`) whose LOCAL bound name is `std`/`core`, and an `extern crate … as
/// std|core;` rename. The full-path trait-impl allowlist
/// ([`MACRO_ARG_PRODUCER_ALLOWED_TRAIT_IMPL_PATHS`]) lists the qualified spellings
/// `std::clone::Clone` / `std::fmt::Debug` / `core::…`; rebinding `std`/`core`
/// would let a FOREIGN trait be SPELLED as an allowlisted `std::clone::Clone` path
/// and slip through the syntactic allowlist. Banning the rebind keeps those
/// qualified std-spellings SEMANTICALLY exact (a normal `use std::fmt;` binds
/// `fmt`, not `std`, so it is unaffected).
///
/// The genuine `macro_arg_producer.rs` imports (`std::cell::Cell`,
/// `std::sync::{Arc, OnceLock}`, `rustc_hash::FxHashMap`, `verter_type_expr::{…}`,
/// `crate::…`, and the fn-local `verter_session_query::analysis::types::AnalyzedMacroKind`)
/// bind NONE of the built-in-derive names and use no glob, so they pass. Skips
/// `#[cfg(test)]`-gated items; recurses into non-test inline modules and fn-body
/// `use` statements.
pub(super) fn macro_arg_producer_derive_shadow_import_violations(src: &str) -> Vec<String> {
    use syn::visit::Visit;
    let file = match syn::parse_file(src) {
        Ok(f) => f,
        Err(e) => return vec![format!("parse error: {e}")],
    };
    struct DeriveShadowVisitor {
        violations: Vec<String>,
    }
    impl DeriveShadowVisitor {
        /// Walk a `use`-tree, collecting every leaf-bound name and flagging a
        /// glob. `prefix` accumulates the path for diagnostics.
        fn walk_use_tree(&mut self, tree: &syn::UseTree, prefix: &str) {
            match tree {
                syn::UseTree::Path(p) => {
                    let next = if prefix.is_empty() {
                        p.ident.to_string()
                    } else {
                        format!("{prefix}::{}", p.ident)
                    };
                    self.walk_use_tree(&p.tree, &next);
                }
                syn::UseTree::Group(g) => {
                    for item in &g.items {
                        self.walk_use_tree(item, prefix);
                    }
                }
                syn::UseTree::Glob(_) => {
                    self.violations.push(format!(
                        "a production glob import `use {prefix}::*;` is forbidden in the producer \
                         module — it could bring a built-in-derive name into scope under a foreign \
                         definition (derive-shadow)"
                    ));
                }
                leaf @ (syn::UseTree::Name(_) | syn::UseTree::Rename(_)) => {
                    if let Some(bound) = use_leaf_bound_name(leaf) {
                        if MACRO_ARG_PRODUCER_BUILTIN_DERIVES.contains(&bound.as_str()) {
                            self.violations.push(format!(
                                "a production import binding `{bound}` (under `{prefix}`) is \
                                 forbidden in the producer module — it shadows the compiler \
                                 built-in derive `{bound}` so a `#[derive({bound})]` could resolve \
                                 to a foreign macro (derive-shadow)"
                            ));
                        }
                        // CRATE-ROOT REBIND: a `use … as std;` / `use … as core;`
                        // (or a single-segment `use std;` / `use core;`) binds the
                        // LOCAL name `std`/`core` to something other than the genuine
                        // crate root. The full-path trait-impl allowlist
                        // (`MACRO_ARG_PRODUCER_ALLOWED_TRAIT_IMPL_PATHS`) lists the
                        // qualified spellings `std::clone::Clone` / `std::fmt::Debug` /
                        // `core::…`; once `std`/`core` is rebound, a FOREIGN trait could
                        // be SPELLED as an allowlisted `std::clone::Clone` and pass the
                        // syntactic allowlist. Banning the rebind keeps the qualified
                        // std-spellings SEMANTICALLY exact, so the allowlist is not
                        // merely syntactic. (A normal `use std::fmt;` binds `fmt`, `use
                        // std::clone::Clone;` binds `Clone` — neither binds `std`, so
                        // neither is affected.)
                        if bound == "std" || bound == "core" {
                            self.violations.push(format!(
                                "a production import binding the crate-root name `{bound}` (under \
                                 `{prefix}`) is forbidden in the producer module — a `use … as \
                                 {bound};` rebinds the crate root the full-path trait-impl \
                                 allowlist's qualified `{bound}::…` spellings depend on, so a \
                                 foreign trait could be spelled as an allowlisted `{bound}::…` path \
                                 (crate-root-rebind)"
                            ));
                        }
                    }
                }
            }
        }
    }
    impl<'ast> syn::visit::Visit<'ast> for DeriveShadowVisitor {
        fn visit_item_use(&mut self, u: &'ast syn::ItemUse) {
            if attrs_test_or_test_support_gate(&u.attrs) {
                return;
            }
            self.walk_use_tree(&u.tree, "");
            syn::visit::visit_item_use(self, u);
        }
        // A fn-local `use` is a `syn::Stmt::Item(Item::Use(_))` — `visit_item_use`
        // fires for it via the default item walk, so no extra override is needed.
        fn visit_attribute(&mut self, attr: &'ast syn::Attribute) {
            if attr.path().is_ident("macro_use") {
                self.violations.push(
                    "a production `#[macro_use]` attribute is forbidden in the producer module — \
                     it can inject foreign derive macros that shadow the built-ins (derive-shadow)"
                        .to_string(),
                );
            }
            syn::visit::visit_attribute(self, attr);
        }
        fn visit_item_extern_crate(&mut self, ec: &'ast syn::ItemExternCrate) {
            // The overridden `visit_item` does NOT skip a `#[cfg(test)]` extern-crate
            // (its attrs match omits `ExternCrate`), so this override gates cfg-test
            // itself — matching `visit_item_use`'s `attrs_test_gate` treatment.
            if attrs_test_or_test_support_gate(&ec.attrs) {
                return;
            }
            // CRATE-ROOT REBIND via extern-crate rename: `extern crate evil as std;`
            // (or `as core;`) rebinds the crate-root name the full-path trait-impl
            // allowlist's qualified `std::…` / `core::…` spellings depend on, so a
            // foreign trait could be spelled as an allowlisted `std::clone::Clone`
            // path. Only the rename TO `std`/`core` is this evasion — a bare `extern
            // crate foo;` (no rename) binds `foo`, not `std`/`core`, and is unaffected.
            if let Some((_as, rename)) = &ec.rename {
                let bound = unraw_ident(&rename.to_string()).to_string();
                if bound == "std" || bound == "core" {
                    self.violations.push(format!(
                        "a production `extern crate {} as {bound};` is forbidden in the producer \
                         module — it rebinds the crate-root name `{bound}` the full-path trait-impl \
                         allowlist's qualified `{bound}::…` spellings depend on, so a foreign trait \
                         could be spelled as an allowlisted `{bound}::…` path (crate-root-rebind)",
                        ec.ident
                    ));
                }
            }
            syn::visit::visit_item_extern_crate(self, ec);
        }
        fn visit_item(&mut self, item: &'ast syn::Item) {
            // Skip a `#[cfg(test)]`-gated item wholesale (its imports are
            // test-only).
            let attrs: &[syn::Attribute] = match item {
                syn::Item::Use(u) => &u.attrs,
                syn::Item::Mod(m) => &m.attrs,
                syn::Item::Fn(f) => &f.attrs,
                syn::Item::Impl(i) => &i.attrs,
                syn::Item::Struct(s) => &s.attrs,
                syn::Item::Enum(e) => &e.attrs,
                _ => &[],
            };
            if attrs.iter().any(attr_is_exactly_cfg_test) {
                return;
            }
            syn::visit::visit_item(self, item);
        }
    }
    let mut v = DeriveShadowVisitor {
        violations: Vec::new(),
    };
    v.visit_file(&file);
    v.violations
}

/// The macro-arg producer SURFACE files — the ONLY owner-module files exempt
/// from the eager-macro-arg-lowering ordering tripwire. They legitimately read
/// a macro's `parsed_type_argument` (or, for the binder-seed child, a
/// script-setup generic's constraint/default) and lower it through the ONE
/// shared structural lowerer while building the mode-neutral mirror handle.
///
/// The exemption is SCOPED to these surface files — NOT the whole
/// `structural_carrier_producer/` directory: the raw lowerer (`lower.rs`) does
/// no macro-arg reading, and a future owner file (e.g. a decl-body producer
/// surface) must not silently inherit a blanket exemption that would hide an
/// eager-lowering regression there. Path tail (not anchored) so the check is
/// OS-portable — `session_production_src_files()` normalises separators to `/`.
pub(super) const MACRO_ARG_PRODUCER_SURFACE_EXEMPT_FILES: &[&str] =
    &["structural_carrier_producer/macro_arg_producer.rs"];

/// Whether `rel` is the sanctioned macro-arg producer SURFACE file exempt from
/// the eager-macro-arg-lowering ordering tripwire. ONLY the single producer
/// module `macro_arg_producer.rs` (which owns the macro hot mirror builder and
/// the binder-seed builder) is exempt; every other file in the owner directory
/// — and everywhere else in production — is in scope.
pub(super) fn macro_arg_producer_surface_is_exempt(rel: &str) -> bool {
    MACRO_ARG_PRODUCER_SURFACE_EXEMPT_FILES
        .iter()
        .any(|exempt| rel.ends_with(exempt))
}

/// SECONDARY ordering tripwire (codex-flagged distinct migration invariant):
/// a NON-TEST `verter_session` production FILE that BOTH reads an
/// `AnalyzedMacro.parsed_type_argument` AND lowers via
/// `lower_type_expr_in_scope_with_*` OUTSIDE the macro hot mirror
/// producer/accessor is a forbidden second macro-arg eager-lowering path. The
/// four converted macro sites now read the ONE mirror; the out-of-scope sites
/// lower OTHER exprs (not `parsed_type_argument`).
///
/// This is a FILE-SCOPE ORDERING TRIPWIRE, NOT a dataflow proof — the primary
/// mechanism is the visibility on the structural lowerer above. It catches a
/// forbidden pairing in EITHER of two shapes:
///
///  1. WHOLE-FUNCTION conjunction: both tokens inside ONE function body
///     (catches a pairing split more than 12 lines apart within a function the old
///     adjacency window missed).
///  2. CROSS-FUNCTION binding-flow (the GOV-flagged helper split): a binding
///     bound from `parsed_type_argument` in one function is later handed to an
///     eager `lower_type_expr_in_scope_with_*` call ANYWHERE in the file
///     (regardless of which function). This catches a helper split that the
///     whole-function conjunction misses, WITHOUT false-positiving on a file
///     (e.g. `vue_exec/mod.rs`) where a `parsed_type_argument` presence-guard
///     read in one fn co-exists with an UNRELATED eager lowering of a non-macro
///     `param_ty` in another fn — the flow check requires the lowered subject
///     to be the macro-arg-derived binding, not merely co-present.
pub(super) fn macro_arg_eager_lowering_violations(rel: &str, body: &str) -> bool {
    // The single producer module (`macro_arg_producer.rs`) is the sanctioned
    // macro-arg producer/accessor home — it legitimately reads a macro's
    // `parsed_type_argument` (and a script-setup generic's constraint/default)
    // and lowers it through the module-private structural lowerer while building
    // the ONE mode-neutral mirror handle. ONLY that module is exempt — NOT the
    // whole owner directory: any future owner file (e.g. a decl-body producer
    // surface) must NOT inherit a blanket exemption, so an eager macro-arg
    // lowering planted there is still a violation.
    if macro_arg_producer_surface_is_exempt(rel) {
        return false;
    }
    // Comment-stripped, cfg(test)-stripped production body (mirrors
    // `session_production_ident_hits`' scan precision so a stale doc-comment
    // mention or a test-gated body is not a hit). Comments are blanked FIRST so
    // a brace inside a doc comment cannot corrupt the function-extent brace
    // scan below.
    let production_body = carrier_strip_comments(&strip_cfg_test_gated_source(body));

    // SHAPE 1 — WHOLE-FUNCTION CONJUNCTION: both tokens co-present inside ONE
    // function body.
    let whole_fn = fn_bodies_in_stripped_source(&production_body)
        .iter()
        .any(|fn_body| {
            let has_macro_arg = fn_body.contains("parsed_type_argument");
            let has_eager = fn_body.contains("lower_type_expr_in_scope_with_mode(")
                || fn_body.contains("lower_type_expr_in_scope_with_context(");
            has_macro_arg && has_eager
        });
    if whole_fn {
        return true;
    }

    // SHAPE 2 — CROSS-FUNCTION binding-flow (file-scope): a `let X = …
    // parsed_type_argument …` binding whose name `X` is later an argument to an
    // eager `lower_type_expr_in_scope_with_*(…)` call anywhere in the file.
    macro_arg_binding_flows_to_eager_lowering(&production_body)
}

/// Detect the GOV-flagged helper-split evasion at FILE SCOPE: an identifier
/// bound from a `parsed_type_argument` read (`let X … = … parsed_type_argument
/// …;`) that is later passed as an argument to an eager
/// `lower_type_expr_in_scope_with_*( … X … )` call ANYWHERE in the
/// (comment/cfg-test-stripped) file.
///
/// This is the precise discriminator between the violation and the legitimate
/// `vue_exec/mod.rs` collision: there, the eager call lowers a function
/// parameter `param_ty` that is NEVER bound from `parsed_type_argument`, so no
/// macro-arg-derived binding flows into it.
pub(super) fn macro_arg_binding_flows_to_eager_lowering(stripped_src: &str) -> bool {
    // 1) Collect identifiers bound from a `parsed_type_argument` read. The read
    //    can flow into a binding through several common shapes — ALL of which
    //    GOV P0 #2 requires the extractor to catch:
    //      * a plain `let X = … parsed_type_argument …;`
    //      * a let-else `let Some(arg) = …parsed_type_argument….as_ref() else {…};`
    //      * an `if let Some(arg) = …parsed_type_argument…` / `while let …`
    //      * a `match …parsed_type_argument… { … }` whose ARM patterns bind the
    //        macro-arg value (e.g. `Some(arg) => …`)
    //      * any `.as_ref()` / `.clone()` / `?`-chained read of the field
    //    The binding identifiers are extracted from the LHS pattern (every
    //    binder in it, so `Some(arg)` / `(a, b)` are covered, not just the first
    //    token), and match-arm binders are pulled from the arms of a match whose
    //    scrutinee reads `parsed_type_argument`.
    let mut macro_arg_bindings: Vec<String> = Vec::new();
    let src_lines: Vec<&str> = stripped_src.lines().collect();
    for (line_idx, line) in src_lines.iter().enumerate() {
        let trimmed = line.trim_start();

        // (a) match over a `parsed_type_argument` read: collect arm-pattern
        //     binders from the following lines until the match block closes (a
        //     simple brace-depth walk over arm lines). The value the arms bind
        //     IS the macro-arg value.
        if trimmed.starts_with("match ")
            && line_before_block_brace(trimmed).contains("parsed_type_argument")
        {
            collect_match_arm_binders(&src_lines, line_idx, &mut macro_arg_bindings);
            continue;
        }

        if !line.contains("parsed_type_argument") {
            continue;
        }

        // (b) binding forms: `let …`, `let … else`, `if let …`, `while let …`.
        //     Extract the LHS pattern (everything between the `let`/`if let`/
        //     `while let` keyword and the FIRST top-level `=` that opens the
        //     RHS) and pull EVERY binder identifier from it.
        let pattern_region = trimmed
            .strip_prefix("let ")
            .or_else(|| trimmed.strip_prefix("if let "))
            .or_else(|| trimmed.strip_prefix("while let "));
        let Some(pattern_region) = pattern_region else {
            continue;
        };
        // The pattern is everything up to the first `=` (the binding operator).
        // A `==` would not appear in a binding LHS, and a type annotation `:`
        // also terminates the pattern.
        let pat = pattern_region
            .split('=')
            .next()
            .unwrap_or("")
            .split(':')
            .next()
            .unwrap_or("");
        collect_pattern_binders(pat, &mut macro_arg_bindings);
    }
    if macro_arg_bindings.is_empty() {
        return false;
    }

    // 2) Extract the argument text of every eager-lowering call in the file and
    //    check whether any macro-arg binding appears as a whole-ident argument.
    for call_args in eager_lowering_call_arg_spans(stripped_src) {
        for binding in &macro_arg_bindings {
            if ident_appears_whole(&call_args, binding) {
                return true;
            }
        }
    }
    false
}

/// Return the portion of a `match …` line BEFORE the block-opening `{` (the
/// scrutinee expression). `match foo.parsed_type_argument.as_ref() {` →
/// `match foo.parsed_type_argument.as_ref() `. Used so a `match` whose
/// scrutinee reads `parsed_type_argument` is recognised even when the `{` is on
/// the same line.
pub(super) fn line_before_block_brace(line: &str) -> &str {
    line.split_once('{').map(|(head, _)| head).unwrap_or(line)
}

/// From a `match` at `match_line_idx` whose scrutinee reads
/// `parsed_type_argument`, collect the binder identifiers introduced by its arm
/// patterns (`Some(arg) => …`, `Ok(v) => …`, `(a, b) => …`). The arms bind the
/// MACRO-ARG value, so those binders are macro-arg-derived. A brace-depth walk
/// scopes collection to the match block (the `{` after the scrutinee through its
/// matching `}`); each arm line's pattern is the text left of `=>`.
pub(super) fn collect_match_arm_binders(
    lines: &[&str],
    match_line_idx: usize,
    out: &mut Vec<String>,
) {
    // Find the block-opening `{` (this line or a following one) and walk to the
    // matching close, collecting `PAT =>` binders inside depth 1.
    let mut depth = 0i32;
    let mut started = false;
    for line in lines.iter().skip(match_line_idx) {
        for ch in line.chars() {
            match ch {
                '{' => {
                    depth += 1;
                    started = true;
                }
                '}' => depth -= 1,
                _ => {}
            }
        }
        // Inside the match block (depth ≥ 1): an arm line is `PATTERN => …`.
        if started && depth >= 1 {
            if let Some((pat, _)) = line.split_once("=>") {
                collect_pattern_binders(pat, out);
            }
        }
        if started && depth <= 0 {
            break;
        }
    }
}

/// Extract every binder identifier from a Rust pattern fragment, skipping
/// keywords (`mut` / `ref`) and PascalCase constructor / variant names
/// (`Some` / `Ok` / `None` — a leading uppercase letter marks a constructor,
/// not a binder). `Some(arg)` → `["arg"]`; `(a, b)` → `["a", "b"]`;
/// `Some(ref mut x)` → `["x"]`; `_` is dropped. Over-collection of a
/// lowercase variant-less token is harmless: the value-flow check only fires
/// when that token also appears as a whole-ident eager-call argument.
pub(super) fn collect_pattern_binders(pat: &str, out: &mut Vec<String>) {
    // Tokenise into ident-runs; keep lowercase/underscore-leading binders.
    let mut token = String::new();
    let flush = |token: &mut String, out: &mut Vec<String>| {
        if token.is_empty() {
            return;
        }
        let t = std::mem::take(token);
        // Skip keywords and the `_` wildcard.
        if matches!(t.as_str(), "mut" | "ref" | "_") {
            return;
        }
        // A PascalCase leading char marks a constructor / variant / struct
        // name (e.g. `Some`, `Ok`, `RowApi`), never a binder.
        if t.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
            return;
        }
        out.push(t);
    };
    for c in pat.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            token.push(c);
        } else {
            flush(&mut token, out);
        }
    }
    flush(&mut token, out);
}

/// Return the parenthesised argument text of every
/// `lower_type_expr_in_scope_with_mode(` / `..._with_context(` call in the
/// (already comment/cfg-test-stripped) source. Paren-depth + string/char-literal
/// aware so a `)` inside a nested call or a literal does not truncate the span.
pub(super) fn eager_lowering_call_arg_spans(src: &str) -> Vec<String> {
    const CALLS: &[&str] = &[
        "lower_type_expr_in_scope_with_mode(",
        "lower_type_expr_in_scope_with_context(",
    ];
    let bytes = src.as_bytes();
    let mut spans: Vec<String> = Vec::new();
    for call in CALLS {
        let mut from = 0usize;
        while let Some(rel_idx) = src[from..].find(call) {
            let open = from + rel_idx + call.len(); // first byte INSIDE the parens
                                                    // Walk to the matching close paren (literal-aware), depth starts at 1.
            let mut depth = 1i32;
            let mut k = open;
            let mut in_str = false;
            let mut in_char = false;
            while k < bytes.len() {
                let c = bytes[k];
                if in_str {
                    if c == b'\\' {
                        k += 2;
                        continue;
                    }
                    if c == b'"' {
                        in_str = false;
                    }
                } else if in_char {
                    if c == b'\\' {
                        k += 2;
                        continue;
                    }
                    if c == b'\'' {
                        in_char = false;
                    }
                } else {
                    match c {
                        b'"' => in_str = true,
                        b'\'' => in_char = true,
                        b'(' => depth += 1,
                        b')' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                k += 1;
            }
            let end = k.min(bytes.len());
            spans.push(src[open..end].to_string());
            from = open;
        }
    }
    spans
}

/// Whole-identifier (word-boundary) containment: `needle` appears in `haystack`
/// not as a substring of a larger identifier.
pub(super) fn ident_appears_whole(haystack: &str, needle: &str) -> bool {
    let hb = haystack.as_bytes();
    let nb = needle.as_bytes();
    if nb.is_empty() {
        return false;
    }
    let mut i = 0usize;
    while let Some(rel) = haystack[i..].find(needle) {
        let start = i + rel;
        let end = start + nb.len();
        let before_ok = start == 0 || !is_ident_byte(hb[start - 1]);
        let after_ok = end >= hb.len() || !is_ident_byte(hb[end]);
        if before_ok && after_ok {
            return true;
        }
        i = start + 1;
    }
    false
}

/// Split a comment/cfg-test-stripped Rust source into per-function bodies by
/// brace-depth tracking (string/char-literal aware). Each returned slice spans
/// one top-level-or-nested `fn` from its `{` through its matching `}`. A
/// presence-guard read and an eager lowering in DIFFERENT functions therefore
/// land in DIFFERENT slices. Used by the macro-arg ordering tripwire to scope
/// the conjunction to one function body.
pub(super) fn fn_bodies_in_stripped_source(src: &str) -> Vec<String> {
    let bytes = src.as_bytes();
    let mut bodies: Vec<String> = Vec::new();
    let mut i = 0usize;
    // A simple literal-aware brace scanner. On finding an `fn` keyword
    // boundary, locate its opening `{` and capture through the matching `}`.
    while i < bytes.len() {
        // Detect an `fn` token at a word boundary (byte-level, UTF-8 safe — the
        // body may carry multibyte chars inside doc comments / string
        // literals).
        let is_fn = bytes[i] == b'f'
            && bytes.get(i + 1) == Some(&b'n')
            && (i == 0 || !is_ident_byte(bytes[i - 1]))
            && bytes.get(i + 2).is_none_or(|&b| !is_ident_byte(b));
        if !is_fn {
            i += 1;
            continue;
        }
        // Find the opening brace of this fn (skip its signature; bail if a `;`
        // at paren-depth 0 ends a bodiless fn decl before any `{`).
        let mut j = i + 2;
        let mut paren = 0i32;
        let mut open = None;
        while j < bytes.len() {
            match bytes[j] {
                b'(' => paren += 1,
                b')' => paren -= 1,
                b'{' if paren <= 0 => {
                    open = Some(j);
                    break;
                }
                b';' if paren <= 0 => break,
                _ => {}
            }
            j += 1;
        }
        let Some(open) = open else {
            i += 2;
            continue;
        };
        // Capture through the matching close brace (string/char-literal aware).
        let mut depth = 0i32;
        let mut k = open;
        let mut in_str = false;
        let mut in_char = false;
        let mut end = bytes.len();
        while k < bytes.len() {
            let c = bytes[k];
            if in_str {
                if c == b'\\' {
                    k += 2;
                    continue;
                }
                if c == b'"' {
                    in_str = false;
                }
            } else if in_char {
                if c == b'\\' {
                    k += 2;
                    continue;
                }
                if c == b'\'' {
                    in_char = false;
                }
            } else {
                match c {
                    b'"' => in_str = true,
                    b'\'' => in_char = true,
                    b'{' => depth += 1,
                    b'}' => {
                        depth -= 1;
                        if depth == 0 {
                            end = k + 1;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            k += 1;
        }
        bodies.push(src[open..end].to_string());
        i = end;
    }
    bodies
}

pub(super) fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Walk every non-test `verter_session/src/**.rs` and
/// `verter_type_engine/src/**.rs` production file, returning
/// `(workspace-relative path, body)` pairs. Skips `*_tests.rs` / `tests.rs` /
/// `typeinfo_tests/` (mirrors the production scan scope in
/// [`session_production_ident_hits`]).
pub(super) fn session_production_src_files() -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for crate_dir in [
        "crates/verter_session/src/",
        "crates/verter_type_engine/src/",
    ] {
        let crate_root = workspace_path(crate_dir.trim_end_matches('/'));
        for entry in walkdir::WalkDir::new(&crate_root)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|e| e.path().is_file())
        {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let path_str = path.to_string_lossy().replace('\\', "/");
            if path_str.ends_with("_tests.rs")
                || path_str.ends_with("/tests.rs")
                || path_str.contains("/tests/")
                || path_str.contains("/typeinfo_tests/")
            {
                continue;
            }
            if let Ok(body) = std::fs::read_to_string(path) {
                let rel = path_str
                    .rsplit_once(crate_dir)
                    .map(|(_, s)| format!("{crate_dir}{s}"))
                    .unwrap_or(path_str);
                out.push((rel, body));
            }
        }
    }
    assert!(
        out.len() > 100,
        "guard scanner found only {} production files — the walk is broken",
        out.len()
    );
    out
}

/// The macro hot mirror is a PURE producer: it lowers a macro
/// `parsed_type_argument` into the dormant structural carrier graph with NO
/// host route lookup and NO dependency emission — resolution + dep recording
/// happen at the resolving DEMAND, never as a side-band preflight on the
/// producer. A route-resolving host call inside the mirror module (the
/// owner's prepared-decl bundle, an import-route resolution, a route type-edge
/// resolution) would route-resolve imports inside the builder, re-introducing
/// the dep-omission regression class. Script-setup generic seeding therefore
/// re-sources its binders from the owner's ROUTE-FREE local `IndexedReady`
/// data (`raw_source` + `framework_parse` via `sfc_script_setup_type_params`),
/// never from the prepared-decl bundle.
///
/// This is a source-scan tripwire over the mirror module's non-test
/// production source (mirroring `session_production_ident_hits` precision:
/// comment-stripped, cfg(test)-stripped).
pub(super) fn macro_hot_mirror_impurity_hits(body: &str) -> Vec<String> {
    // The full route/import/cross-file-symbol-resolution surface a future
    // impurity could use to route-resolve inside the pure producer. These are
    // the `ResolverContext` / host methods that resolve imports, routes,
    // cross-file symbols, dependency canonicals, OR carrier heads — NOT the
    // route-free shallow reads the mirror legitimately uses
    // (`indexed_ready*` / `shallow_file_state` / `sfc_script_setup_type_params`
    // / `local_type_declaration_id` same-file lookup / `dispatch_node_data`
    // arena read). `contains`-matching means a banned prefix also catches its
    // `_shallow` sibling (e.g. `resolve_named_type_export_target` ⇒
    // `resolve_named_type_export_target_shallow`).
    const ROUTE_RESOLVING_IDENTS: &[&str] = &[
        // host route/import-route bundles + the per-symbol prepared-decl cache
        // accessors (`ResolverContext::prepared_type_decl` /
        // `prepared_value_decl`): these route through the SAME prepared-decl
        // context path `prepared_decl_bundle` reaches, so a producer touching
        // any of them re-introduces the route-resolution-inside-the-builder
        // regression class. The route-free shallow reads the mirror DOES use
        // (`indexed_ready*` / `shallow_file_state` / `sfc_script_setup_type_params`
        // / `local_type_declaration_id` / arena `dispatch_node_data`) are NOT
        // here.
        "prepared_decl_bundle",
        "prepared_type_decl",
        "prepared_value_decl",
        "cached_import_route_resolution",
        "resolve_route_type_edge",
        // [P0] the eager dispatch/query route — the second resolution engine the
        // single-engine rule forbids. A producer reaching
        // `ctx.dispatch().lower_type_expr_in_scope_with_context(...)` route-
        // resolves imports through the prepared-decl path; both the `.dispatch(`
        // method call and the eager-lowering entry are banned. The `.dispatch(`
        // open-paren form does NOT match the allowed route-free arena read
        // `dispatch_node_data(` (whose chars after `dispatch` are `_node_data(`,
        // not `(`).
        ".dispatch(",
        "lower_type_expr_in_scope_with_",
        // The route-resolving `ensure_indexed_ready(` (open-paren) — DISTINCT from
        // the allowed route-free `ensure_indexed_ready_serve(` the mirror accessor
        // calls (its char after the prefix is `_`, never `(`, so the needle never
        // matches it).
        "ensure_indexed_ready(",
        // ResolverContext "Symbol / route resolution" surface
        "resolve_imported_type_root",
        "resolve_named_type_export_target",
        "resolve_owner_direct_import",
        "resolve_type_dependency_canonical",
        "routed_shallow_state",
        "resolve_type_declaration_for_dep",
        "resolve_value_export_target",
        // carrier-head / bare-name resolution (a producer must NOT resolve)
        "resolve_bare_ref_head",
        "resolve_import_type_head",
        "resolve_carrier_subject_node",
        "resolve_bare_name_in_scope",
    ];
    ident_hits_in_production_body(body, ROUTE_RESOLVING_IDENTS)
        .into_iter()
        .map(|(_, ident)| ident)
        .collect()
}

/// The SANCTIONED crate-visible entries of the structural-carrier producer
/// module.
///
/// - `macro_type_arg_hot_ref` — the ONE public entry whose private
///   implementation is the module-private shared structural lowerer
///   (`lower_type_expr_structural` in `macro_arg_producer.rs`).
/// - `attach` — mints the mirror's opaque `MacroMirrorAttachment` storage
///   handle (a clone of the shared cell array). It carries no lowering work,
///   so it is NOT a producer entry; it is listed here so the entry-surface
///   guard reports the module's REAL crate-visible surface rather than
///   exempting it by matching its body spelling. What keeps it work-free is
///   the capability guards over `macro_arg_producer.rs`
///   (`macro_hot_mirror_producer_is_pure_no_route_resolution`,
///   `session_graph_lowerer_makes_no_query`), which ban the whole
///   query / dispatch / resolution surface from the module — a name-keyed
///   body-shape classifier would only have re-stated the same property for
///   one method.
///
/// No OTHER crate-visible fn may appear in the owner module — a second
/// producer-capable entry would be a new outward producer entry. The
/// structural lowerer, the macro hot-mirror builder, and the binder-seed
/// builder are ALL module-private (no visibility modifier), so they never
/// appear in this crate-visible scan.
pub(super) const MIRROR_SANCTIONED_CRATE_VISIBLE_ENTRIES: &[&str] =
    &["macro_type_arg_hot_ref", "attach"];

/// Whether `name` is a sanctioned structural-carrier-producer crate-visible
/// entry — the macro-arg accessor or the attachment mint. Every other
/// producer-capable fn is module-private to `macro_arg_producer.rs`
/// (compiler-confined), so it can never be crate-visible.
pub(super) fn mirror_entry_is_sanctioned(name: &str) -> bool {
    MIRROR_SANCTIONED_CRATE_VISIBLE_ENTRIES.contains(&name)
}

/// Whether an attribute list test-gates an item — i.e. its `#[cfg]` predicate
/// ENTAILS `test` (every configuration that satisfies the predicate has
/// `test = true`), so the item is compiled ONLY under a test build and never
/// widens the crate-visible PRODUCTION producer surface.
///
/// SATISFIABILITY/ENTAILMENT classification (the [P0] root closure). The prior
/// classifier treated `test` appearing in ANY `any(...)` arm as test-gating, so
/// `#[cfg(any(test, debug_assertions))]` was over-EXCLUDED even though it is
/// PRODUCTION-SATISFIABLE (a debug non-test build satisfies it via
/// `debug_assertions`) — a rogue `#[cfg(any(test, debug_assertions))] pub(crate)
/// fn` would compile into the always-on debug production lib and evade the
/// single-entry guard. The corrected rule ([`predicate_entails_test`]):
/// - a bare `test` atom ENTAILS test; any other atom (`debug_assertions`,
///   `unix`, …) and a `feature = "…"` name-value do NOT;
/// - `all(P1..Pn)` entails test iff ANY operand entails test (one required
///   operand forcing test forces the whole conjunction);
/// - `any(P1..Pn)` entails test iff ALL operands entail test (otherwise some
///   satisfying configuration omits test);
/// - `not(P)` entails test iff `P` is UNSATISFIABLE-without-test would mean the
///   negation forces test — but in the cfg fragment we model, `not(test)` is
///   satisfied by every non-test config, and `not(X)` for a non-test atom never
///   forces test, so `not(...)` NEVER entails test under this conservative
///   classifier (a `not(test)` item is PRODUCTION and is COUNTED).
///
/// So `#[cfg(test)]` and `#[cfg(all(test, unix))]` are test-only;
/// `#[cfg(any(test, debug_assertions))]`, `#[cfg(any(test, feature = "x"))]`,
/// `#[cfg(not(test))]`, and a bare `#[cfg(debug_assertions)]` are PRODUCTION and
/// are COUNTED.
///
/// `#[cfg_attr(...)]` is NOT an item gate and is IGNORED here. `#[cfg_attr(cond,
/// X)]` CONDITIONALLY APPLIES attribute `X` when `cond` holds — it NEVER removes
/// the ITEM from the build; the item is compiled in EVERY configuration. Treating
/// `cfg_attr` like `cfg` (classifying the item test-gated when its condition
/// entails test) was UNSOUND: `#[cfg_attr(test, allow(dead_code))] pub(crate) fn
/// rogue()` is a real production producer entry (compiled into the non-test lib)
/// yet was wrongly excluded — a single-entry evasion across all three collectors
/// that call this helper. So `attrs_test_gate` gates on `#[cfg(...)]` ONLY. (A
/// nested `#[cfg_attr(test, cfg(...))]` injecting a real `cfg` is exotic and the
/// expansion-surface guard already bans every production `cfg_attr`; the sound
/// rule here is simply that `cfg_attr` never test-gates the item.)
pub(super) fn attrs_test_gate(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        let path = attr.path();
        // ONLY `#[cfg(...)]` gates an item out of a build. `#[cfg_attr(...)]`
        // conditionally applies an attribute and never excludes the item, so it
        // is not an item gate (see the doc comment above).
        if !path.is_ident("cfg") {
            return false;
        }
        let tokens = match &attr.meta {
            syn::Meta::List(list) => list.tokens.clone(),
            // `#[cfg]` / `#[cfg = …]` with no predicate list: never test-gating.
            _ => return false,
        };
        // The `#[cfg(...)]` body is a SINGLE predicate (one operand). Classify it.
        match cfg_split_top_level_operands(tokens).into_iter().next() {
            Some(pred) => predicate_entails_test(pred),
            None => false,
        }
    })
}

/// [`attrs_test_gate`] for the structural-carrier-producer seal guards, which
/// additionally count an item gated on the `test-support` feature as test code:
/// that feature exists only for test targets and no shipped artifact enables
/// it, so a `#[cfg(any(test, feature = "test-support"))]` seam never widens a
/// shipped build's producer surface. Every other feature, `debug_assertions`,
/// `not(...)` and `cfg_attr` keep their [`attrs_test_gate`] classification.
pub(super) fn attrs_test_or_test_support_gate(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        if !attr.path().is_ident("cfg") {
            return false;
        }
        let tokens = match &attr.meta {
            syn::Meta::List(list) => list.tokens.clone(),
            _ => return false,
        };
        match cfg_split_top_level_operands(tokens).into_iter().next() {
            Some(pred) => predicate_entails_test_or_test_support(pred),
            None => false,
        }
    })
}

/// [`predicate_entails_test`] with the `feature = "test-support"` atom also
/// entailing test.
pub(super) fn predicate_entails_test_or_test_support(pred: proc_macro2::TokenStream) -> bool {
    use proc_macro2::TokenTree;
    let trees: Vec<TokenTree> = pred.into_iter().collect();
    if let (Some(TokenTree::Ident(head)), Some(TokenTree::Group(group))) =
        (trees.first(), trees.get(1))
    {
        let inner_operands = cfg_split_top_level_operands(group.stream());
        return match head.to_string().as_str() {
            "all" => inner_operands
                .into_iter()
                .any(predicate_entails_test_or_test_support),
            "any" => {
                !inner_operands.is_empty()
                    && inner_operands
                        .into_iter()
                        .all(predicate_entails_test_or_test_support)
            }
            _ => false,
        };
    }
    match trees.as_slice() {
        [TokenTree::Ident(id)] => *id == "test",
        [TokenTree::Ident(key), TokenTree::Punct(eq), TokenTree::Literal(value)] => {
            *key == "feature" && eq.as_char() == '=' && value.to_string() == "\"test-support\""
        }
        _ => false,
    }
}

/// Split a cfg token stream into its TOP-LEVEL comma-separated operands. The
/// operands of `all(...)` / `any(...)` are separated by top-level commas; a
/// comma INSIDE a nested group (e.g. inside `not(any(a, b))`) belongs to that
/// nested predicate and is NOT a split point (it lives inside a `Group`'s own
/// stream, so the flat top-level walk never sees it). A trailing comma yields no
/// empty operand.
pub(super) fn cfg_split_top_level_operands(
    tokens: proc_macro2::TokenStream,
) -> Vec<proc_macro2::TokenStream> {
    use proc_macro2::{TokenStream, TokenTree};
    let mut operands: Vec<TokenStream> = Vec::new();
    let mut current: Vec<TokenTree> = Vec::new();
    for tt in tokens {
        match &tt {
            TokenTree::Punct(p) if p.as_char() == ',' => {
                if !current.is_empty() {
                    operands.push(current.drain(..).collect());
                }
            }
            _ => current.push(tt),
        }
    }
    if !current.is_empty() {
        operands.push(current.into_iter().collect());
    }
    operands
}

/// Whether a SINGLE cfg predicate (one top-level operand) ENTAILS `test` — every
/// configuration satisfying it has `test = true`. See [`attrs_test_gate`] for the
/// full entailment rule set. The predicate is one of: a bare atom (`test` /
/// `debug_assertions` / `unix` / …), a name-value (`feature = "x"`), or a
/// combinator `all(...)` / `any(...)` / `not(...)`.
pub(super) fn predicate_entails_test(pred: proc_macro2::TokenStream) -> bool {
    use proc_macro2::TokenTree;
    let trees: Vec<TokenTree> = pred.into_iter().collect();
    // A combinator predicate is `Ident(all|any|not) Group[...]`.
    if let (Some(TokenTree::Ident(head)), Some(TokenTree::Group(group))) =
        (trees.first(), trees.get(1))
    {
        let inner_operands = cfg_split_top_level_operands(group.stream());
        return match head.to_string().as_str() {
            // `all(...)` entails test iff ANY operand entails test.
            "all" => inner_operands.into_iter().any(predicate_entails_test),
            // `any(...)` entails test iff ALL operands entail test. An EMPTY
            // `any()` is unsatisfiable (entails everything vacuously) — but it is
            // not a real shape; `all(true)` over the empty set is `true`, so an
            // empty `any()` returning `true` here is conservatively test-only,
            // never widening the production surface incorrectly (there is no such
            // item). We treat empty as NOT test-gating to avoid spurious
            // exclusion of a degenerate shape.
            "any" => {
                !inner_operands.is_empty() && inner_operands.into_iter().all(predicate_entails_test)
            }
            // `not(P)`: in the cfg fragment we model, a negation never FORCES
            // test — `not(test)` is satisfied by every non-test config (so it is
            // production), and `not(non-test)` never forces test either. So
            // `not(...)` does not entail test (the item is COUNTED).
            "not" => false,
            // Any other call-shaped predicate we do not model is treated
            // conservatively as NOT entailing test (so the item is COUNTED — a
            // production producer is never silently excluded).
            _ => false,
        };
    }
    // A bare atom: only the single ident `test` entails test. `debug_assertions`,
    // `unix`, and a `feature = "…"` name-value do NOT.
    matches!(trees.as_slice(), [TokenTree::Ident(id)] if *id == "test")
}

/// Whether a [`syn::Visibility`] is crate-visible OUTWARD — reachable by a
/// caller in an arbitrary OTHER module of THIS crate. A bare `pub` (in fact
/// wider) is crate-visible; so is a restricted path of EXACTLY `crate`, spelled
/// `pub(crate)` OR `pub(in crate)` — Rust treats the two as identical crate-wide
/// visibility. An inherited (module-private) visibility and any DEEPER restricted
/// form (`pub(in crate::structural_carrier_producer)` / other `pub(in crate::…)` subtree
/// path / `pub(super)` / `pub(self)`) are NOT crate-visible: they cannot be named
/// from an arbitrary other production module to act as a second outward entry.
pub(super) fn visibility_is_crate_visible(vis: &syn::Visibility) -> bool {
    match vis {
        // Bare `pub` — crate-visible (and wider).
        syn::Visibility::Public(_) => true,
        // A restricted path of EXACTLY `crate` is crate-wide visibility WHETHER
        // spelled `pub(crate)` (no `in` token) or `pub(in crate)` (with the `in`
        // token) — both name the crate root, so both are crate-visible. Any
        // DEEPER path (`pub(in crate::structural_carrier_producer)`, `pub(in crate::a::b)`),
        // `pub(super)`, and `pub(self)` are restricted BELOW crate scope and are
        // NOT crate-visible.
        syn::Visibility::Restricted(r) => r.path.is_ident("crate"),
        // Inherited (module-private) — not crate-visible.
        syn::Visibility::Inherited => false,
    }
}

/// Collect crate-visible (`pub` / `pub(crate)` / `pub(in crate)`)
/// PRODUCER-CAPABLE function names in a Rust source — BOTH module-level FREE
/// functions (`ItemFn`) AND ASSOCIATED functions inside INHERENT `impl` blocks
/// (`ImplItemFn`), recursing through inline `mod` blocks. Either shape is an
/// outward producer entry a foreign crate-scope caller can reach, so the mirror
/// entry-surface guard must enumerate both.
///
/// TRAIT-impl methods are NOT a producer-entry vector and are intentionally
/// SKIPPED: a trait-impl method inherits the trait's visibility and cannot carry
/// its own `pub` / `pub(crate)` (`impl T for X { pub(crate) fn … }` is not valid
/// Rust), so it can never be an INDEPENDENTLY crate-visible outward producer
/// entry. The guard's real exposure surface is therefore crate-visible FREE fns
/// + crate-visible INHERENT-impl associated fns.
///
/// Exclusions:
/// - A `pub(in crate::…)`-subtree / `pub(super)` / `pub(self)` restricted entry
///   is NOT crate-visible (see [`visibility_is_crate_visible`]); `pub(in crate)`
///   (exact crate root) IS.
/// - An item, impl, or module whose `#[cfg(...)]` ENTAILS test — `#[cfg(test)]`
///   or `#[cfg(all(test, …))]` — is test-only and never widens the production
///   producer surface (see [`attrs_test_gate`]). A PRODUCTION-satisfiable gate is
///   NOT excluded: `#[cfg(any(test, …))]`, `#[cfg(not(test))]`, and a bare
///   `#[cfg(debug_assertions)]` all compile into a non-test build and ARE counted.
///
/// GOV P0 (round 4): the previous line-scanner only saw module-level FREE fns
/// (brace-depth 0) and MISSED a crate-visible ASSOCIATED fn — e.g.
/// `impl Foo { pub(crate) fn second_entry(...) { … } }` — which is equally a
/// second outward producer entry. The `syn`-based walk recognises a producer
/// entry regardless of whether it is free or associated, and regardless of any
/// `unsafe` / `async` / `const` / `extern "…"` modifier (carried structurally
/// on `sig`, so no modifier text-skipping is needed).
pub(super) fn crate_visible_producer_fn_names(src: &str) -> Vec<String> {
    let file = syn::parse_file(src).unwrap_or_else(|e| panic!("crate-visible scan parse: {e}"));
    let mut names: Vec<String> = Vec::new();
    collect_crate_visible_fns_in_items(&file.items, &mut names);
    names
}

/// Recursive worker for [`crate_visible_producer_fn_names`]: walk a slice of
/// items, collecting crate-visible free fns + `impl`-block associated fns and
/// descending into non-test inline modules.
pub(super) fn collect_crate_visible_fns_in_items(items: &[syn::Item], names: &mut Vec<String>) {
    for item in items {
        match item {
            // A module-level free function.
            syn::Item::Fn(f) => {
                if attrs_test_or_test_support_gate(&f.attrs) {
                    continue;
                }
                if visibility_is_crate_visible(&f.vis) {
                    names.push(f.sig.ident.to_string());
                }
            }
            // INHERENT `impl` block — every crate-visible associated fn inside
            // is an outward producer entry. A TRAIT `impl` (`imp.trait_` set) is
            // SKIPPED: its methods inherit the trait's visibility and can never
            // be an independently crate-visible producer entry, so they are not
            // a producer-entry vector. A test-gated impl is skipped wholesale.
            syn::Item::Impl(imp) => {
                if imp.trait_.is_some() {
                    continue;
                }
                if attrs_test_or_test_support_gate(&imp.attrs) {
                    continue;
                }
                for impl_item in &imp.items {
                    if let syn::ImplItem::Fn(m) = impl_item {
                        if attrs_test_or_test_support_gate(&m.attrs) {
                            continue;
                        }
                        if visibility_is_crate_visible(&m.vis) {
                            names.push(m.sig.ident.to_string());
                        }
                    }
                }
            }
            // Inline module — descend (skipping a test-gated module such as the
            // `for_tests` facade) so a producer entry can't hide one level in.
            syn::Item::Mod(m) => {
                if attrs_test_or_test_support_gate(&m.attrs) {
                    continue;
                }
                if let Some((_, inner)) = &m.content {
                    collect_crate_visible_fns_in_items(inner, names);
                }
            }
            _ => {}
        }
    }
}

/// Whether a `syn::Expr` (or any sub-expression) NAMES one of the producer
/// builders [`STRUCTURAL_CARRIER_PRODUCER_PRIVATE_BUILDERS`] — used to reject a
/// `const` / `static` / associated-const that binds a producer builder as an
/// fn-pointer value (the VALUE-exposure vector: `pub(crate) const F: fn(..) -> _
/// = lower_type_expr_structural;`). A `syn::visit` over path segments catches the
/// builder name wherever it appears in the initialiser expression.
pub(super) fn expr_references_producer_builder(expr: &syn::Expr) -> bool {
    use syn::visit::Visit;
    struct BuilderRefVisitor {
        found: bool,
    }
    impl<'ast> syn::visit::Visit<'ast> for BuilderRefVisitor {
        fn visit_path_segment(&mut self, seg: &'ast syn::PathSegment) {
            let name = seg.ident.to_string();
            if STRUCTURAL_CARRIER_PRODUCER_PRIVATE_BUILDERS.contains(&name.as_str()) {
                self.found = true;
            }
            syn::visit::visit_path_segment(self, seg);
        }
    }
    let mut v = BuilderRefVisitor { found: false };
    v.visit_expr(expr);
    v.found
}

/// Collect VALUE-exposure violations in `macro_arg_producer.rs`: a crate-visible
/// (`pub` / `pub(crate)` / `pub(in crate)`) `const` / `static` item — or a
/// crate-visible associated `const` inside an INHERENT `impl` — whose initialiser
/// NAMES a producer builder (an fn-pointer / closure value that re-exports the
/// builder through a value binding). Such a binding lets an arbitrary other module
/// reach the builder through the const/static, re-opening the producer surface
/// WITHOUT a `fn`-item. Skips `#[cfg(test)]`-gated items (entailment-classified
/// via [`attrs_test_gate`]). Recurses into non-test inline modules.
pub(super) fn macro_arg_producer_value_exposure_violations(src: &str) -> Vec<String> {
    let file = match syn::parse_file(src) {
        Ok(f) => f,
        Err(e) => return vec![format!("parse error: {e}")],
    };
    let mut out: Vec<String> = Vec::new();
    collect_value_exposure_in_items(&file.items, &mut out);
    out
}

pub(super) fn collect_value_exposure_in_items(items: &[syn::Item], out: &mut Vec<String>) {
    for item in items {
        match item {
            syn::Item::Const(c) => {
                if attrs_test_or_test_support_gate(&c.attrs) {
                    continue;
                }
                if visibility_is_crate_visible(&c.vis) && expr_references_producer_builder(&c.expr)
                {
                    out.push(format!(
                        "a crate-visible `const {}` binds a producer builder as a value — a \
                         value-exposure re-export of the module-private builder",
                        c.ident
                    ));
                }
            }
            syn::Item::Static(s) => {
                if attrs_test_or_test_support_gate(&s.attrs) {
                    continue;
                }
                if visibility_is_crate_visible(&s.vis) && expr_references_producer_builder(&s.expr)
                {
                    out.push(format!(
                        "a crate-visible `static {}` binds a producer builder as a value — a \
                         value-exposure re-export of the module-private builder",
                        s.ident
                    ));
                }
            }
            // Inherent-impl associated consts: a crate-visible associated const
            // holding a producer fn-pointer is equally a value-exposure vector.
            syn::Item::Impl(imp) => {
                if imp.trait_.is_some() || attrs_test_or_test_support_gate(&imp.attrs) {
                    continue;
                }
                for impl_item in &imp.items {
                    if let syn::ImplItem::Const(c) = impl_item {
                        if attrs_test_or_test_support_gate(&c.attrs) {
                            continue;
                        }
                        if visibility_is_crate_visible(&c.vis)
                            && expr_references_producer_builder(&c.expr)
                        {
                            out.push(format!(
                                "a crate-visible associated `const {}` binds a producer builder as \
                                 a value — a value-exposure re-export of the module-private builder",
                                c.ident
                            ));
                        }
                    }
                }
            }
            syn::Item::Mod(m) => {
                if attrs_test_or_test_support_gate(&m.attrs) {
                    continue;
                }
                if let Some((_, inner)) = &m.content {
                    collect_value_exposure_in_items(inner, out);
                }
            }
            _ => {}
        }
    }
}

/// The hand-written trait impls `macro_arg_producer.rs` legitimately carries —
/// EXACTLY `Debug` and `Clone` for `MacroHotMirror` (the struct also
/// `#[derive(Default)]`s, which is an ATTRIBUTE handled by the expansion-surface
/// derive check, not a trait-impl item). Any OTHER trait impl in the producer
/// module is a TRAIT-EXPOSURE vector (a trait-dispatch surface that could reach a
/// producer-capable method) and is rejected.
///
/// The allowlist is keyed by the FULL trait path spelling, NOT just the final
/// segment: a QUALIFIED `impl evil::Clone for MacroHotMirror` (where `evil::Clone`
/// is a USER-DEFINED trait merely NAMED `Clone`, not `core::clone::Clone` — no
/// coherence collision with the real hand-written `impl Clone`, so it COMPILES)
/// must NOT be admitted by a final-segment match. The allowed spellings cover the
/// KNOWN-GOOD exact std paths only: `Debug` is written qualified as
/// `std::fmt::Debug` in the real file (so the bare `Debug` plus the two std
/// spellings are admitted), and `Clone` is written bare (so `Clone` plus the two
/// std spellings are admitted). Anything else — including a deeper/foreign
/// qualified path like `evil::Clone` or `evil::Debug` — is rejected. The self type
/// must be `MacroHotMirror`.
///
/// The qualified `std::…` / `core::…` spellings in this allowlist are kept
/// SEMANTICALLY exact (not merely syntactic) by the companion crate-root-rebind
/// rule in [`macro_arg_producer_derive_shadow_import_violations`], which bans
/// rebinding the local names `std`/`core` in the producer module (via a `use … as
/// std;` / `use … as core;` alias OR an `extern crate … as std|core;` rename).
/// Without that companion ban, a same-module rebind (`use evil as std;`) followed
/// by `impl std::clone::Clone for MacroHotMirror {}` would SPELL as an allowlisted
/// path while resolving to a FOREIGN trait; with the rebind banned, `std`/`core`
/// necessarily resolve to the genuine crate roots, so these spellings are exact.
pub(super) const MACRO_ARG_PRODUCER_ALLOWED_TRAIT_IMPL_PATHS: &[(&[&str], &str)] = &[
    // Debug — bare or the std spellings.
    (&["Debug"], "MacroHotMirror"),
    (&["std", "fmt", "Debug"], "MacroHotMirror"),
    (&["core", "fmt", "Debug"], "MacroHotMirror"),
    // Clone — bare or the std spellings.
    (&["Clone"], "MacroHotMirror"),
    (&["std", "clone", "Clone"], "MacroHotMirror"),
    (&["core", "clone", "Clone"], "MacroHotMirror"),
];

/// The leading-colon-stripped segment idents of a trait path
/// (`std::fmt::Debug` → `["std", "fmt", "Debug"]`, `Clone` → `["Clone"]`).
pub(super) fn trait_path_segments(trait_path: &syn::Path) -> Vec<String> {
    trait_path
        .segments
        .iter()
        .map(|s| s.ident.to_string())
        .collect()
}

/// Whether `(trait_path, self_name)` is an EXACT allowlisted hand-written trait
/// impl — the full trait-path segments AND the self type must match an
/// allowlist row. A multi-segment foreign path (`evil::Clone`) never matches a
/// bare `["Clone"]` row, and a non-std qualified path never matches a std row.
pub(super) fn trait_impl_is_allowlisted(trait_path: &syn::Path, self_name: Option<&str>) -> bool {
    let segs = trait_path_segments(trait_path);
    MACRO_ARG_PRODUCER_ALLOWED_TRAIT_IMPL_PATHS
        .iter()
        .any(|(allowed_segs, allowed_self)| {
            segs.len() == allowed_segs.len()
                && segs
                    .iter()
                    .zip(allowed_segs.iter())
                    .all(|(got, want)| got == want)
                && Some(*allowed_self) == self_name
        })
}

/// Collect TRAIT-exposure violations in `macro_arg_producer.rs`:
/// - any NON-allowlisted trait impl (the allowlist is the hand-written
///   `Debug` / `Clone` for `MacroHotMirror`);
/// - an allowlisted trait impl whose body NAMES a producer builder (a
///   trait-dispatch path reaching a builder);
/// - any crate-visible (or any) trait DEFINITION or trait ALIAS in the module
///   (a crate-visible trait could expose a producer-capable default method).
///
/// Skips `#[cfg(test)]`-gated items. Recurses into non-test inline modules.
///
/// CONFINEMENT BOUNDARY (closed realistic surface + documented theoretical residual).
/// The inline-`mod std`/`mod core` trait-shadow class is closed by three layers: the
/// FOREIGN case is compiler-confined (E0603 — a foreign module cannot name the bare
/// module-private lowerer); the SAME-MODULE inline-`mod`-with-a-LOCAL-trait-definition
/// case is rejected here (this collector recurses into non-test inline modules and bans
/// any `Item::Trait` def/alias, so an inline `mod std { trait LocalClone … }` reddens on
/// the local def regardless of the module name); and the strengthened same-module
/// builders/use/attr guards close the rebind/re-export variants. The ONE remaining residual
/// — an inline `mod std`/`mod core` re-exporting an EXTERNAL crate's `clone::Clone` /
/// `fmt::Debug` with NO local trait def — is a documented THEORETICAL, insider-only
/// residual: it requires a crafted `Cargo.toml` dependency exposing a bespoke
/// `clone::Clone`/`fmt::Debug` module shape AND an inline `mod std` placed inside the
/// trusted private producer module, far beyond an accidental contributor duplication. It
/// is accepted as relying on review rather than escalating the source scanner further
/// (the producer-confinement record lives in `.claude/skills/type-resolution/SKILL.md`).
pub(super) fn macro_arg_producer_trait_exposure_violations(src: &str) -> Vec<String> {
    let file = match syn::parse_file(src) {
        Ok(f) => f,
        Err(e) => return vec![format!("parse error: {e}")],
    };
    let mut out: Vec<String> = Vec::new();
    collect_trait_exposure_in_items(&file.items, &mut out);
    out
}

pub(super) fn collect_trait_exposure_in_items(items: &[syn::Item], out: &mut Vec<String>) {
    for item in items {
        match item {
            // A trait IMPL — `impl Trait for Type`.
            syn::Item::Impl(imp) => {
                if attrs_test_or_test_support_gate(&imp.attrs) {
                    continue;
                }
                if let Some((_, trait_path, _)) = &imp.trait_ {
                    let trait_spelling = trait_path_segments(trait_path).join("::");
                    let self_name = type_path_last_segment(&imp.self_ty);
                    // FULL-PATH allowlist: a final-segment match would wrongly admit
                    // a foreign `evil::Clone`. Match the exact std spellings instead.
                    if !trait_impl_is_allowlisted(trait_path, self_name.as_deref()) {
                        out.push(format!(
                            "a non-allowlisted production trait impl `impl {trait_spelling} for {}` \
                             is forbidden — only the hand-written `Debug` / `Clone` (bare or std \
                             spelling) for `MacroHotMirror` are allowed; a qualified foreign-trait \
                             path (e.g. `evil::Clone`) is NOT the std trait and a trait impl is a \
                             dispatch surface that could expose a producer-capable method",
                            self_name.as_deref().unwrap_or("<?>")
                        ));
                        continue;
                    }
                    // Allowlisted impl: its body must NOT name a producer builder —
                    // through a METHOD body OR an associated CONST initialiser (an
                    // assoc-const fn-pointer like `const F: fn(..) = builder;` inside
                    // an allowlisted impl is equally a trait-dispatch reach to a
                    // builder).
                    for impl_item in &imp.items {
                        match impl_item {
                            syn::ImplItem::Fn(m) => {
                                let block_expr = syn::Expr::Block(syn::ExprBlock {
                                    attrs: Vec::new(),
                                    label: None,
                                    block: m.block.clone(),
                                });
                                if expr_references_producer_builder(&block_expr) {
                                    out.push(format!(
                                        "the allowlisted trait impl `impl {trait_spelling} for \
                                         MacroHotMirror`'s method `{}` NAMES a producer builder — a \
                                         trait method must never reach a lowering builder",
                                        m.sig.ident
                                    ));
                                }
                            }
                            syn::ImplItem::Const(c)
                                if expr_references_producer_builder(&c.expr) =>
                            {
                                out.push(format!(
                                    "the allowlisted trait impl `impl {trait_spelling} for \
                                     MacroHotMirror`'s associated `const {}` NAMES a producer \
                                     builder as a value — an associated const fn-pointer must \
                                     never reach a lowering builder",
                                    c.ident
                                ));
                            }
                            _ => {}
                        }
                    }
                }
            }
            // A trait DEFINITION or trait ALIAS — a crate-visible trait could
            // expose a producer-capable default method through dispatch; even a
            // module-private trait is an unexpected surface here, so ANY trait
            // def/alias in the producer module is rejected.
            syn::Item::Trait(t) => {
                if attrs_test_or_test_support_gate(&t.attrs) {
                    continue;
                }
                out.push(format!(
                    "a production trait DEFINITION `trait {}` is forbidden in the producer module \
                     — a trait is a dispatch surface that could expose a producer-capable method",
                    t.ident
                ));
            }
            syn::Item::TraitAlias(t) => {
                if attrs_test_or_test_support_gate(&t.attrs) {
                    continue;
                }
                out.push(format!(
                    "a production trait ALIAS `trait {} = …` is forbidden in the producer module",
                    t.ident
                ));
            }
            syn::Item::Mod(m) => {
                if attrs_test_or_test_support_gate(&m.attrs) {
                    continue;
                }
                if let Some((_, inner)) = &m.content {
                    collect_trait_exposure_in_items(inner, out);
                }
            }
            _ => {}
        }
    }
}

/// The FINAL path segment of a `syn::Type` if it is a plain type path
/// (`MacroHotMirror`, `crate::…::MacroHotMirror`) — `None` for a non-path type.
pub(super) fn type_path_last_segment(ty: &syn::Type) -> Option<String> {
    if let syn::Type::Path(tp) = ty {
        return tp.path.segments.last().map(|s| s.ident.to_string());
    }
    None
}

/// The EXACT sanctioned shape of `structural_carrier_producer/mod.rs`: it must
/// declare the typed-IR helper as a PRIVATE `mod infer_binder_names;`, declare
/// the producer module as a PRIVATE `mod macro_arg_producer;` (no visibility),
/// and re-export EXACTLY
/// `pub(crate) use macro_arg_producer::{macro_type_arg_hot_ref, MacroHotMirror};`
/// — no aliases, no globs, no extra leaves, no re-exported restricted
/// helpers/traits/consts, no `pub`-widened module decl. The helper is a sibling,
/// so it cannot name the producer child's private builders. Returns the list of
/// violations (empty on the genuine `mod.rs`). A `syn` walk over the file's
/// items enforces the shape structurally (not a substring scan).
pub(super) fn mod_rs_reexport_shape_violations(src: &str) -> Vec<String> {
    let file = match syn::parse_file(src) {
        Ok(f) => f,
        Err(e) => return vec![format!("parse error: {e}")],
    };
    let mut out: Vec<String> = Vec::new();
    let mut saw_helper_decl = false;
    let mut saw_mod_decl = false;
    let mut reexported: Vec<String> = Vec::new();
    for item in &file.items {
        match item {
            // The typed-IR walker is the one sanctioned non-producer sibling.
            // It must stay private, attribute-free, and out-of-line.
            syn::Item::Mod(m) if m.ident == "infer_binder_names" => {
                saw_helper_decl = true;
                if !matches!(m.vis, syn::Visibility::Inherited) {
                    out.push(
                        "`mod infer_binder_names;` must be PRIVATE — widening it is outside the \
                         sanctioned typed-IR helper boundary"
                            .to_string(),
                    );
                }
                if !m.attrs.is_empty() {
                    out.push(
                        "`mod infer_binder_names;` must carry no attributes — `#[path]` or an \
                         attribute macro could substitute a producer-capable implementation"
                            .to_string(),
                    );
                }
                if m.content.is_some() {
                    out.push(
                        "`mod infer_binder_names` must be an out-of-line declaration \
                         (`mod infer_binder_names;`)"
                            .to_string(),
                    );
                }
            }
            // The `mod macro_arg_producer;` declaration must be PRIVATE and
            // body-less (out-of-line into the sibling file).
            syn::Item::Mod(m) if m.ident == "macro_arg_producer" => {
                saw_mod_decl = true;
                if !matches!(m.vis, syn::Visibility::Inherited) {
                    out.push(
                        "`mod macro_arg_producer;` must be PRIVATE (no `pub`/`pub(crate)`) — a \
                         `pub`/`pub(crate)` module decl would let a foreign module name the \
                         producer's private builders"
                            .to_string(),
                    );
                }
                // The `mod macro_arg_producer;` decl must carry NO attribute other
                // than (optionally) a STRICT `#[cfg(test)]` gate. A `#[path =
                // "../evil.rs"]` re-roots the module to an arbitrary source file (a
                // build-substitution route) while keeping the inherited visibility +
                // out-of-line shape, and any attribute proc-macro could rewrite the
                // decl. The strict `attr_is_exactly_cfg_test` recognizer is used
                // (NOT `attrs_test_gate`, consistent with the test-wiring path) so a
                // `cfg_attr` / production-satisfiable cfg / `#[path]` is rejected.
                for attr in &m.attrs {
                    if attr_is_exactly_cfg_test(attr) {
                        continue;
                    }
                    let attr_path = attr
                        .path()
                        .segments
                        .iter()
                        .map(|s| s.ident.to_string())
                        .collect::<Vec<_>>()
                        .join("::");
                    out.push(format!(
                        "`mod macro_arg_producer;` carries a forbidden attribute `#[{attr_path}]` — \
                         the sanctioned decl carries NO attribute other than a strict `#[cfg(test)]`; \
                         a `#[path = …]` re-roots the producer module to an arbitrary file and a \
                         proc-macro attribute could rewrite it"
                    ));
                }
                if m.content.is_some() {
                    out.push(
                        "`mod macro_arg_producer` must be an out-of-line declaration (`mod \
                         macro_arg_producer;`), not an inline `mod macro_arg_producer { … }`"
                            .to_string(),
                    );
                }
            }
            // Any OTHER module declaration in mod.rs is an unexpected producer
            // surface.
            syn::Item::Mod(m) => {
                out.push(format!(
                    "mod.rs declares an unexpected module `mod {}` — the owner root must declare \
                     ONLY the private typed-IR helper and producer modules",
                    m.ident
                ));
            }
            // A `use` item: the ONLY sanctioned one is
            // `pub(crate) use macro_arg_producer::{macro_type_arg_hot_ref, MacroHotMirror};`.
            // A test-gated (`test` / `test-support`) re-export is test wiring,
            // never compiled into a shipped artifact.
            syn::Item::Use(u) if attrs_test_or_test_support_gate(&u.attrs) => {}
            syn::Item::Use(u) => match mod_rs_use_violation(u) {
                Err(reason) => out.push(reason),
                Ok(leaves) => reexported.extend(leaves),
            },
            // Any other item in mod.rs (a fn, const, trait, struct, impl, …) is an
            // unexpected surface in the owner root.
            other => {
                out.push(format!(
                    "mod.rs contains an unexpected item ({}) — the owner root must hold ONLY the \
                     private helper/producer module declarations and the one sanctioned \
                     producer re-export",
                    describe_item_kind(other)
                ));
            }
        }
    }
    if !saw_helper_decl {
        out.push("mod.rs must declare `mod infer_binder_names;`".to_string());
    }
    if !saw_mod_decl {
        out.push("mod.rs must declare `mod macro_arg_producer;`".to_string());
    }
    reexported.sort();
    let required_present = ["MacroHotMirror", "macro_type_arg_hot_ref"]
        .iter()
        .all(|leaf| reexported.iter().any(|r| r == leaf));
    let unique = reexported.windows(2).all(|pair| pair[0] != pair[1]);
    if !required_present || !unique {
        out.push(format!(
            "mod.rs must re-export EXACTLY `macro_type_arg_hot_ref` (crate-private) and \
             `MacroHotMirror`, each once, with only optional owned `MacroHotProduct` / opaque \
             `MacroMirrorAttachment` nominal leaves; found {reexported:?}"
        ));
    }
    out
}

/// Check one mod.rs re-export: `macro_arg_producer::<leaf>` or a group of
/// bare leaves drawn ONLY from the sole producer function and the mirror
/// nominals (`MacroHotMirror`, owned `MacroHotProduct`, opaque
/// `MacroMirrorAttachment`). The producer function stays crate-private
/// (`pub(crate)`); the nominals may be `pub(crate)` or `pub` (the host crate
/// names the mirror types). Wrong visibility/root, aliases, globs, nested
/// paths, unknown leaves and private builder exports are violations. Returns
/// the statement's leaves.
pub(super) fn mod_rs_use_violation(u: &syn::ItemUse) -> Result<Vec<String>, String> {
    let crate_private = match &u.vis {
        syn::Visibility::Restricted(r) => r.path.is_ident("crate") && r.in_token.is_none(),
        _ => false,
    };
    let public = matches!(u.vis, syn::Visibility::Public(_));
    if !crate_private && !public {
        return Err(format!(
            "the mod.rs re-export must be `pub(crate) use …` or `pub use …`; found a different \
             visibility on `use {}`",
            quote_use_tree(&u.tree)
        ));
    }
    let syn::UseTree::Path(path) = &u.tree else {
        return Err("the mod.rs re-export root must be `macro_arg_producer::…`".to_string());
    };
    if path.ident != "macro_arg_producer" {
        return Err(format!(
            "the mod.rs re-export root must be `macro_arg_producer`, found `{}`",
            path.ident
        ));
    }
    let leaf_trees: Vec<&syn::UseTree> = match &*path.tree {
        syn::UseTree::Group(group) => group.items.iter().collect(),
        single => vec![single],
    };
    let mut leaves: Vec<String> = Vec::new();
    for leaf in leaf_trees {
        match leaf {
            syn::UseTree::Name(n) => leaves.push(n.ident.to_string()),
            syn::UseTree::Rename(r) => {
                return Err(format!(
                    "the mod.rs re-export must not alias — found `{} as {}`",
                    r.ident, r.rename
                ));
            }
            syn::UseTree::Glob(_) => {
                return Err("the mod.rs re-export must not use a glob `*`".to_string());
            }
            syn::UseTree::Path(p) => {
                return Err(format!(
                    "the mod.rs re-export must not nest a path — found `{}::…`",
                    p.ident
                ));
            }
            syn::UseTree::Group(_) => {
                return Err("the mod.rs re-export must not nest a group".to_string());
            }
        }
    }
    let nominals = ["MacroHotMirror", "MacroHotProduct", "MacroMirrorAttachment"];
    for leaf in &leaves {
        if leaf == "macro_type_arg_hot_ref" {
            if !crate_private {
                return Err(
                    "the sole producer function `macro_type_arg_hot_ref` must be re-exported \
                     `pub(crate)` — never wider"
                        .to_string(),
                );
            }
        } else if !nominals.contains(&leaf.as_str()) {
            return Err(format!(
                "the mod.rs re-export leaves must be drawn ONLY from `macro_type_arg_hot_ref` and \
                 the mirror nominals {nominals:?}; found `{leaf}`"
            ));
        }
    }
    Ok(leaves)
}

/// Render a `syn::UseTree` to a compact string for diagnostics.
pub(super) fn quote_use_tree(tree: &syn::UseTree) -> String {
    use quote::ToTokens;
    tree.to_token_stream().to_string()
}

/// A short human label for a `syn::Item` kind, for the mod.rs unexpected-item
/// diagnostic.
pub(super) fn describe_item_kind(item: &syn::Item) -> &'static str {
    match item {
        syn::Item::Fn(_) => "a fn",
        syn::Item::Const(_) => "a const",
        syn::Item::Static(_) => "a static",
        syn::Item::Trait(_) => "a trait",
        syn::Item::TraitAlias(_) => "a trait alias",
        syn::Item::Struct(_) => "a struct",
        syn::Item::Enum(_) => "an enum",
        syn::Item::Impl(_) => "an impl",
        syn::Item::Type(_) => "a type alias",
        syn::Item::Macro(_) => "a macro",
        syn::Item::ExternCrate(_) => "an extern crate",
        syn::Item::ForeignMod(_) => "a foreign mod",
        syn::Item::Union(_) => "a union",
        _ => "an item",
    }
}
