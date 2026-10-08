//! EdgeStore unit tests — sub- (33 tests after R5: #33 deleted, #34 added).
//!
//! Tests are written against the new R4/R5 `DependencySnapshot` model with
//! per-class writers (`replace_parsed_edges`, `replace_exact_resolutions`,
//! `add_lazy_resolved_dep`, `replace_ambient_resolved`,
//! `add_ambient_resolved_dep`, `replace_semantic_transitive`) and the
//! two-axis reverse graph (`reverse_deps_for_target`).

use super::*;
use crate::types::ExactResolution;
use std::collections::BTreeSet;
use verter_session_query::resolution::{ResolutionContext, ResolvePhase, ResolveRequestKind};

fn default_ctx() -> ResolutionContext {
    ResolutionContext {
        phase: ResolvePhase::CodegenBlocker,
        kind: ResolveRequestKind::EsmImport,
    }
}

fn exact(specifier: &str, resolved: Option<&str>, possible: Vec<&str>) -> ExactResolution {
    ExactResolution {
        specifier: specifier.to_string(),
        phase: ResolvePhase::CodegenBlocker,
        kind: ResolveRequestKind::EsmImport,
        resolved_canonical_id: resolved.map(|s| s.to_string()),
        possible_canonical_ids: possible.into_iter().map(|s| s.to_string()).collect(),
    }
}

fn exact_with(
    specifier: &str,
    phase: ResolvePhase,
    kind: ResolveRequestKind,
    resolved: Option<&str>,
) -> ExactResolution {
    ExactResolution {
        specifier: specifier.to_string(),
        phase,
        kind,
        resolved_canonical_id: resolved.map(|s| s.to_string()),
        possible_canonical_ids: vec![],
    }
}

fn btree(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|s| s.to_string()).collect()
}

// ── Test #1 ──
#[test]
fn replace_parsed_edges_records_resolved_in_canonical_axis() {
    let mut store = EdgeStore::new();
    store.replace_parsed_edges("/src/Comp.vue", btree(&["/src/types.ts"]), vec![], vec![]);
    assert_eq!(
        store.reverse_deps_for_target("/src/types.ts", None),
        vec!["/src/Comp.vue"],
    );
}

// ── Test #2 ──
#[test]
fn replace_parsed_edges_records_unresolved_in_stem_axis() {
    let mut store = EdgeStore::new();
    store.replace_parsed_edges(
        "/src/Comp.vue",
        BTreeSet::new(),
        vec![(
            ("./types".to_string(), ResolveRequestKind::EsmImport),
            "/src/types".to_string(),
        )],
        vec![],
    );
    // Querying by stem (no extension stripping needed because stem is bare).
    assert_eq!(
        store.reverse_deps_for_target("/src/types", None),
        vec!["/src/Comp.vue"],
    );
}

// ── Test #3 ──
#[test]
fn replace_parsed_edges_keys_unresolved_by_specifier_and_kind() {
    // F14: same specifier with EsmImport + TypeImport produces two entries.
    let mut store = EdgeStore::new();
    store.replace_parsed_edges(
        "/src/Comp.vue",
        BTreeSet::new(),
        vec![
            (
                ("./types".to_string(), ResolveRequestKind::EsmImport),
                "/src/types".to_string(),
            ),
            (
                ("./types".to_string(), ResolveRequestKind::TypeImport),
                "/src/types".to_string(),
            ),
        ],
        vec![],
    );
    let snap = store.snapshot("/src/Comp.vue").expect("snapshot exists");
    assert_eq!(
        snap.parsed_unresolved_relatives.len(),
        2,
        "two distinct (specifier, kind) entries should coexist for the same specifier"
    );
    assert!(snap
        .parsed_unresolved_relatives
        .contains_key(&("./types".to_string(), ResolveRequestKind::EsmImport)));
    assert!(snap
        .parsed_unresolved_relatives
        .contains_key(&("./types".to_string(), ResolveRequestKind::TypeImport)));
}

// ── Test #4 ──
#[test]
fn byte_identical_replace_parsed_edges_preserves_lazy_resolved() {
    // R22 contract: on byte-identical re-record, secondary classes
    // (here `lazy_resolved`) SURVIVE. The reverse graph is
    // content-addressed; an identical re-record carries no new
    // information and must not poke sibling caches.
    let mut store = EdgeStore::new();
    store.replace_parsed_edges("/src/Comp.vue", BTreeSet::new(), vec![], vec![]);
    store.add_lazy_resolved_dep("/src/Comp.vue", "/node_modules/vue/index.ts");
    assert_eq!(
        store.reverse_deps_for_target("/node_modules/vue/index.ts", None),
        vec!["/src/Comp.vue"],
    );
    // Byte-identical re-record is a TRUE no-op — lazy_resolved is NOT
    // cleared.
    store.replace_parsed_edges("/src/Comp.vue", BTreeSet::new(), vec![], vec![]);
    assert_eq!(
        store.reverse_deps_for_target("/node_modules/vue/index.ts", None),
        vec!["/src/Comp.vue"],
        "R22 contract: byte-identical re-record must NOT clear \
         lazy_resolved (idempotency on the quintuple)"
    );
}

// ── Test #4-bis ──
#[test]
fn structurally_changed_replace_parsed_edges_clears_lazy_resolved() {
    // F11 lifecycle survives the idempotency gate for the structural-change branch:
    // when the parsed-edge inputs ACTUALLY differ, secondary classes
    // are still cleared (parsed re-record is a structural event when
    // the inputs change). This pairs with the byte-identical idempotency
    // test above as the discriminating negation.
    let mut store = EdgeStore::new();
    store.replace_parsed_edges("/src/Comp.vue", BTreeSet::new(), vec![], vec![]);
    store.add_lazy_resolved_dep("/src/Comp.vue", "/node_modules/vue/index.ts");
    assert_eq!(
        store.reverse_deps_for_target("/node_modules/vue/index.ts", None),
        vec!["/src/Comp.vue"],
    );
    // Structural re-record with a DIFFERENT parsed_resolved set →
    // lazy_resolved cleared per F11 lifecycle.
    store.replace_parsed_edges("/src/Comp.vue", btree(&["/lib/types.ts"]), vec![], vec![]);
    assert!(
        store
            .reverse_deps_for_target("/node_modules/vue/index.ts", None)
            .is_empty(),
        "F11 lifecycle: a structurally-changed re-record clears \
         lazy_resolved (parsed_resolved set differs)"
    );
}

// ── Test #5 ──
#[test]
fn byte_identical_replace_parsed_edges_preserves_exact_resolved() {
    // R22 contract: on byte-identical re-record, secondary classes
    // (here `exact_resolved` + `exact_resolutions`) SURVIVE.
    let mut store = EdgeStore::new();
    store.replace_parsed_edges("/src/Comp.vue", BTreeSet::new(), vec![], vec![]);
    store.replace_exact_resolutions(
        "/src/Comp.vue",
        vec![exact("./bar", Some("/src/bar.ts"), vec![])],
    );
    assert_eq!(
        store.reverse_deps_for_target("/src/bar.ts", None),
        vec!["/src/Comp.vue"],
    );
    // Byte-identical re-record is a TRUE no-op — exact_resolved &
    // exact_resolutions are NOT cleared.
    store.replace_parsed_edges("/src/Comp.vue", BTreeSet::new(), vec![], vec![]);
    assert_eq!(
        store.reverse_deps_for_target("/src/bar.ts", None),
        vec!["/src/Comp.vue"],
        "R22 contract: byte-identical re-record must NOT clear \
         exact_resolved"
    );
    assert!(
        store.has_exact_resolutions("/src/Comp.vue"),
        "R22 contract: byte-identical re-record must NOT clear \
         exact_resolutions"
    );
}

// ── Test #5-bis ──
#[test]
fn structurally_changed_replace_parsed_edges_clears_exact_resolved() {
    // F11 lifecycle survives the idempotency gate for the structural-change branch.
    let mut store = EdgeStore::new();
    store.replace_parsed_edges("/src/Comp.vue", BTreeSet::new(), vec![], vec![]);
    store.replace_exact_resolutions(
        "/src/Comp.vue",
        vec![exact("./bar", Some("/src/bar.ts"), vec![])],
    );
    assert_eq!(
        store.reverse_deps_for_target("/src/bar.ts", None),
        vec!["/src/Comp.vue"],
    );
    // Structural re-record clears exact_resolved + exact_resolutions
    // per F11 lifecycle.
    store.replace_parsed_edges("/src/Comp.vue", btree(&["/lib/types.ts"]), vec![], vec![]);
    assert!(
        store
            .reverse_deps_for_target("/src/bar.ts", None)
            .is_empty(),
        "F11 lifecycle: structurally-changed re-record clears \
         exact_resolved"
    );
    assert!(
        !store.has_exact_resolutions("/src/Comp.vue"),
        "F11 lifecycle: structurally-changed re-record clears \
         exact_resolutions"
    );
}

// ── Test #6 ──
#[test]
fn replace_parsed_edges_does_not_clear_ambient_resolved() {
    // F1.5: ambient deps survive parse re-record.
    let mut store = EdgeStore::new();
    store.add_ambient_resolved_dep("/src/Comp.vue", "ambient:/Cabc/lib.es5.d.ts");
    store.replace_parsed_edges("/src/Comp.vue", BTreeSet::new(), vec![], vec![]);
    assert_eq!(
        store.reverse_deps_for_target("ambient:/Cabc/lib.es5.d.ts", None),
        vec!["/src/Comp.vue"],
        "ambient_resolved must SURVIVE parse re-record"
    );
}

// ── Test #7 ──
#[test]
fn byte_identical_replace_parsed_edges_preserves_semantic_transitive() {
    // R22 contract: on byte-identical re-record, `semantic_transitive`
    // SURVIVES. The macro resolver's dep closure is keyed by
    // canonical id; an identical re-record does not change which
    // canonicals are reachable, so the cached transitive edges remain
    // valid.
    let mut store = EdgeStore::new();
    store.replace_parsed_edges("/src/Comp.vue", BTreeSet::new(), vec![], vec![]);
    store.replace_semantic_transitive("/src/Comp.vue", btree(&["/src/shared.ts"]));
    assert_eq!(
        store.reverse_deps_for_target("/src/shared.ts", None),
        vec!["/src/Comp.vue"],
    );
    store.replace_parsed_edges("/src/Comp.vue", BTreeSet::new(), vec![], vec![]);
    assert_eq!(
        store.reverse_deps_for_target("/src/shared.ts", None),
        vec!["/src/Comp.vue"],
        "R22 contract: byte-identical re-record must NOT clear \
         semantic_transitive"
    );
}

// ── Test #7-bis ──
#[test]
fn structurally_changed_replace_parsed_edges_clears_semantic_transitive() {
    // F11 lifecycle survives the idempotency gate for the structural-change branch.
    let mut store = EdgeStore::new();
    store.replace_parsed_edges("/src/Comp.vue", BTreeSet::new(), vec![], vec![]);
    store.replace_semantic_transitive("/src/Comp.vue", btree(&["/src/shared.ts"]));
    assert_eq!(
        store.reverse_deps_for_target("/src/shared.ts", None),
        vec!["/src/Comp.vue"],
    );
    store.replace_parsed_edges("/src/Comp.vue", btree(&["/lib/types.ts"]), vec![], vec![]);
    assert!(
        store
            .reverse_deps_for_target("/src/shared.ts", None)
            .is_empty(),
        "F11 lifecycle: structurally-changed re-record clears \
         semantic_transitive"
    );
}

// ── Test #8 ──
#[test]
fn replace_parsed_edges_replaces_parsed_unresolved_set_symmetrically() {
    let mut store = EdgeStore::new();
    store.replace_parsed_edges(
        "/src/Comp.vue",
        BTreeSet::new(),
        vec![(
            ("./old".to_string(), ResolveRequestKind::EsmImport),
            "/src/old".to_string(),
        )],
        vec![],
    );
    // Re-record with new stem only.
    store.replace_parsed_edges(
        "/src/Comp.vue",
        BTreeSet::new(),
        vec![(
            ("./new".to_string(), ResolveRequestKind::EsmImport),
            "/src/new".to_string(),
        )],
        vec![],
    );
    assert!(
        store.reverse_deps_for_target("/src/old", None).is_empty(),
        "old stem must be removed"
    );
    assert_eq!(
        store.reverse_deps_for_target("/src/new", None),
        vec!["/src/Comp.vue"],
    );
}

// ── Test #9 ──
#[test]
fn replace_exact_resolutions_dampens_matching_active_stem() {
    // F18 active-stem: bundler resolution dampens stem (NOT destroys
    // parsed_unresolved_relatives).
    let mut store = EdgeStore::new();
    store.replace_parsed_edges(
        "/src/Comp.vue",
        BTreeSet::new(),
        vec![(
            ("./types".to_string(), ResolveRequestKind::EsmImport),
            "/src/types".to_string(),
        )],
        vec![],
    );
    assert_eq!(
        store.reverse_deps_for_target("/src/types", None),
        vec!["/src/Comp.vue"],
        "stem present before bundler resolves",
    );
    store.replace_exact_resolutions(
        "/src/Comp.vue",
        vec![exact("./types", Some("/lib/types.ts"), vec![])],
    );
    assert!(
        store.reverse_deps_for_target("/src/types", None).is_empty(),
        "stem must be dampened after bundler resolution"
    );
    assert_eq!(
        store.reverse_deps_for_target("/lib/types.ts", None),
        vec!["/src/Comp.vue"],
        "canonical bucket populated by exact_resolved",
    );
    // Parsed-unresolved entry MUST still be present (R4 active-stem).
    let snap = store.snapshot("/src/Comp.vue").unwrap();
    assert!(
        snap.parsed_unresolved_relatives
            .contains_key(&("./types".to_string(), ResolveRequestKind::EsmImport)),
        "F18: parsed_unresolved_relatives is permanent parser state",
    );
}

// ── Test #10 ──
#[test]
fn replace_exact_resolutions_normalizes_specifier_for_dampening() {
    // F16: `./types` and `./types/` match for dampening.
    let mut store = EdgeStore::new();
    store.replace_parsed_edges(
        "/src/Comp.vue",
        BTreeSet::new(),
        vec![(
            ("./types".to_string(), ResolveRequestKind::EsmImport),
            "/src/types".to_string(),
        )],
        vec![],
    );
    // Bundler passes specifier with trailing slash.
    store.replace_exact_resolutions(
        "/src/Comp.vue",
        vec![exact("./types/", Some("/lib/types.ts"), vec![])],
    );
    assert!(
        store.reverse_deps_for_target("/src/types", None).is_empty(),
        "F16: trailing-slash specifier must dampen the matching stem"
    );
}

// ── Test #11 ──
#[test]
fn replace_exact_resolutions_replaces_canonical_axis_symmetrically() {
    let mut store = EdgeStore::new();
    store.replace_exact_resolutions(
        "/src/Comp.vue",
        vec![exact("./bar", Some("/src/bar.ts"), vec![])],
    );
    assert_eq!(
        store.reverse_deps_for_target("/src/bar.ts", None),
        vec!["/src/Comp.vue"],
    );
    // Re-set with different target.
    store.replace_exact_resolutions(
        "/src/Comp.vue",
        vec![exact("./baz", Some("/src/baz.ts"), vec![])],
    );
    assert!(
        store
            .reverse_deps_for_target("/src/bar.ts", None)
            .is_empty(),
        "old exact target must be removed from canonical axis"
    );
    assert_eq!(
        store.reverse_deps_for_target("/src/baz.ts", None),
        vec!["/src/Comp.vue"],
    );
}

// ── Test #12 ──
#[test]
fn add_lazy_resolved_dep_records_lazy_class_even_when_dep_exists_elsewhere() {
    // R5: idempotency is per-class, not cross-class.
    let mut store = EdgeStore::new();
    store.replace_parsed_edges("/src/Comp.vue", btree(&["/lib/x.ts"]), vec![], vec![]);
    // Even though /lib/x.ts is in parsed_resolved, lazy_resolved still inserts.
    let inserted = store.add_lazy_resolved_dep("/src/Comp.vue", "/lib/x.ts");
    assert!(
        inserted,
        "lazy_resolved must record the dep even when present in parsed_resolved",
    );
    let snap = store.snapshot("/src/Comp.vue").unwrap();
    assert!(snap.parsed_resolved.contains("/lib/x.ts"));
    assert!(snap.lazy_resolved.contains("/lib/x.ts"));
    // Reverse bucket has the owner (union doesn't double-count).
    assert_eq!(
        store.reverse_deps_for_target("/lib/x.ts", None),
        vec!["/src/Comp.vue"],
    );
}

// ── Test #13 ──
#[test]
fn add_ambient_resolved_dep_creates_canonical_reverse_bucket() {
    // F1.5: ambient axis exists.
    let mut store = EdgeStore::new();
    let inserted = store.add_ambient_resolved_dep("/src/Comp.vue", "ambient:/Cabc/lib.es5.d.ts");
    assert!(inserted);
    assert_eq!(
        store.reverse_deps_for_target("ambient:/Cabc/lib.es5.d.ts", None),
        vec!["/src/Comp.vue"],
    );
}

// ── Test #14 ──
#[test]
fn replace_ambient_resolved_replaces_set_symmetrically() {
    let mut store = EdgeStore::new();
    store.replace_ambient_resolved(
        "/src/Comp.vue",
        btree(&["ambient:/A/lib.es5.d.ts", "ambient:/A/lib.dom.d.ts"]),
    );
    assert_eq!(
        store.reverse_deps_for_target("ambient:/A/lib.es5.d.ts", None),
        vec!["/src/Comp.vue"],
    );
    store.replace_ambient_resolved("/src/Comp.vue", btree(&["ambient:/A/lib.dom.d.ts"]));
    assert!(
        store
            .reverse_deps_for_target("ambient:/A/lib.es5.d.ts", None)
            .is_empty(),
        "removed ambient dep must clear reverse bucket"
    );
    assert_eq!(
        store.reverse_deps_for_target("ambient:/A/lib.dom.d.ts", None),
        vec!["/src/Comp.vue"],
    );
}

// ── Test #15 ──
#[test]
fn replace_semantic_transitive_creates_reverse_bucket() {
    let mut store = EdgeStore::new();
    store.replace_semantic_transitive("/src/Comp.vue", btree(&["/lib/shared.ts"]));
    assert_eq!(
        store.reverse_deps_for_target("/lib/shared.ts", None),
        vec!["/src/Comp.vue"],
    );
}

// ── Test #16 ──
#[test]
fn replace_semantic_transitive_replaces_set_symmetrically() {
    let mut store = EdgeStore::new();
    store.replace_semantic_transitive("/src/Comp.vue", btree(&["/lib/old.ts"]));
    store.replace_semantic_transitive("/src/Comp.vue", btree(&["/lib/new.ts"]));
    assert!(
        store
            .reverse_deps_for_target("/lib/old.ts", None)
            .is_empty(),
        "removed transitive dep must clear reverse bucket"
    );
    assert_eq!(
        store.reverse_deps_for_target("/lib/new.ts", None),
        vec!["/src/Comp.vue"],
    );
}

// ── Test #17 ──
#[test]
fn replace_semantic_transitive_handles_promotion_to_direct() {
    // F15: when a transitive dep also becomes direct (parsed), the owner
    // stays in canonical bucket via the union.
    let mut store = EdgeStore::new();
    store.replace_semantic_transitive("/src/Comp.vue", btree(&["/lib/shared.ts"]));
    // Parse re-record (clears semantic_transitive AND records direct).
    store.replace_parsed_edges("/src/Comp.vue", btree(&["/lib/shared.ts"]), vec![], vec![]);
    assert_eq!(
        store.reverse_deps_for_target("/lib/shared.ts", None),
        vec!["/src/Comp.vue"],
        "owner stays in canonical bucket after promotion to direct",
    );
}

// ── Test #18 ──
#[test]
fn reverse_deps_for_target_unions_canonical_and_stem_axes() {
    let mut store = EdgeStore::new();
    // Owner A: canonical hit.
    store.replace_parsed_edges("/src/A.vue", btree(&["/lib/types.ts"]), vec![], vec![]);
    // Owner B: stem hit.
    store.replace_parsed_edges(
        "/src/B.vue",
        BTreeSet::new(),
        vec![(
            ("./other".to_string(), ResolveRequestKind::EsmImport),
            "/lib/types".to_string(),
        )],
        vec![],
    );
    let mut got = store.reverse_deps_for_target("/lib/types.ts", Some("/lib/types"));
    got.sort();
    assert_eq!(
        got,
        vec!["/src/A.vue".to_string(), "/src/B.vue".to_string()],
        "union of canonical and stem axes"
    );
}

// ── Test #19 ──
#[test]
fn reverse_deps_for_target_dedupes_when_owner_in_both_axes() {
    // Same importer in canonical AND stem returns once.
    let mut store = EdgeStore::new();
    store.replace_parsed_edges(
        "/src/Comp.vue",
        btree(&["/lib/types.ts"]),
        vec![(
            ("./types".to_string(), ResolveRequestKind::EsmImport),
            "/lib/types".to_string(),
        )],
        vec![],
    );
    let got = store.reverse_deps_for_target("/lib/types.ts", Some("/lib/types"));
    assert_eq!(
        got,
        vec!["/src/Comp.vue"],
        "owner present in both axes returns once"
    );
}

// ── Test #20 ──
#[test]
fn reverse_deps_for_target_short_circuits_single_axis() {
    // F19: when only one bucket hits, no BTreeSet allocation. We can't
    // assert on allocator behaviour; verify behavioural correctness.
    let mut store = EdgeStore::new();
    store.replace_parsed_edges("/src/Comp.vue", btree(&["/lib/types.ts"]), vec![], vec![]);
    // Only canonical axis hits; stem stripped is `/lib/types` but no stem
    // bucket keyed there.
    let got = store.reverse_deps_for_target("/lib/types.ts", Some("/lib/types"));
    assert_eq!(got, vec!["/src/Comp.vue"]);
}

// ── Test #21 ──
#[test]
fn set_default_resolve_extensions_sorts_longest_first() {
    // F4: sort happens at set-time. We verify by behavioural test through
    // Engine::reverse_deps_for which uses default_resolve_extensions —
    // covered in §4.2 #4 (memory_default_resolve_extensions). Here we
    // verify the strip helper behaviour directly via relative_path.
    let sorted = vec![
        ".d.ts".to_string(),
        ".d.mts".to_string(),
        ".d.cts".to_string(),
        ".tsx".to_string(),
        ".ts".to_string(),
    ];
    // .d.ts (5 chars) must precede .ts (3 chars) in the sorted list.
    let pos_dts = sorted.iter().position(|s| s == ".d.ts").unwrap();
    let pos_ts = sorted.iter().position(|s| s == ".ts").unwrap();
    assert!(
        pos_dts < pos_ts,
        ".d.ts must precede .ts in longest-first sort"
    );
    let stripped = crate::relative_path::strip_extension_first("/types.d.ts", &sorted);
    assert_eq!(stripped, Some("/types"));
}

// ── Test #22 ──
#[test]
fn set_default_resolve_extensions_merges_with_probe_extensions() {
    // F3: workspace merges its own probe list with host config; `.vue` is
    // included (probe contains it); `.tsx` is included (probe contains it,
    // regardless of host config).
    use crate::engine::Engine;
    let engine = Engine::new();
    // Configure with a host-only set that lacks `.vue` and `.tsx`.
    engine.set_default_resolve_extensions(vec![".ts".to_string()]);
    let exts = engine.default_resolve_extensions.load_full();
    assert!(
        exts.iter().any(|e| e == ".vue"),
        ".vue must be merged in from probe_extensions()"
    );
    assert!(
        exts.iter().any(|e| e == ".tsx"),
        ".tsx must be merged in from probe_extensions()"
    );
}

// ── Test #23 ──
#[test]
fn reverse_deps_for_target_strips_d_ts_d_mts_d_cts() {
    // F4: longest-suffix-first stripping for declaration files.
    let sorted: Vec<String> = vec![
        ".d.ts".to_string(),
        ".d.mts".to_string(),
        ".d.cts".to_string(),
        ".tsx".to_string(),
        ".ts".to_string(),
    ];
    assert_eq!(
        crate::relative_path::strip_extension_first("/types.d.ts", &sorted),
        Some("/types")
    );
    assert_eq!(
        crate::relative_path::strip_extension_first("/types.d.mts", &sorted),
        Some("/types")
    );
    assert_eq!(
        crate::relative_path::strip_extension_first("/types.d.cts", &sorted),
        Some("/types")
    );
}

// ── Test #24 ──
#[test]
fn reverse_deps_for_target_known_carrier_outside_resolution_extension_list() {
    // `.svelte` is a KNOWN carrier language (it classifies through its
    // `LanguageRegistry` row), but carrier extensions do not join the
    // import-resolution extension-strip list: a stem bucket never
    // matches a `.svelte` target, so the lookup is canonical-only.
    assert!(
        verter_language::LanguageRegistry::global()
            .classify_static("/src/comp.svelte")
            .static_resolution()
            .is_framework_carrier(),
        ".svelte must classify as a known framework carrier"
    );
    let mut store = EdgeStore::new();
    // Set up a stem bucket for /src/comp (no extension).
    store.replace_parsed_edges(
        "/src/A.vue",
        BTreeSet::new(),
        vec![(
            ("./comp".to_string(), ResolveRequestKind::EsmImport),
            "/src/comp".to_string(),
        )],
        vec![],
    );
    // Querying with `.svelte` (not in the resolution extension list) —
    // only canonical hit. Caller passes `None` for stripped_target since
    // `.svelte` doesn't strip.
    let got = store.reverse_deps_for_target("/src/comp.svelte", None);
    assert!(
        got.is_empty(),
        ".svelte querying must not match a stem bucket"
    );
}

// ── Test #25 ──
#[test]
fn remove_file_surgical_canonical_axis() {
    // M1: surgical via canonical_dep_union.
    let mut store = EdgeStore::new();
    store.replace_parsed_edges(
        "/src/A.vue",
        btree(&["/lib/x.ts", "/lib/y.ts"]),
        vec![],
        vec![],
    );
    store.replace_parsed_edges("/src/B.vue", btree(&["/lib/x.ts"]), vec![], vec![]);
    store.remove_file("/src/A.vue");
    assert_eq!(
        store.reverse_deps_for_target("/lib/x.ts", None),
        vec!["/src/B.vue"],
        "removed owner cleared from /lib/x.ts bucket; B remains"
    );
    assert!(
        store.reverse_deps_for_target("/lib/y.ts", None).is_empty(),
        "/lib/y.ts bucket fully cleared"
    );
}

// ── Test #26 ──
#[test]
fn remove_file_surgical_stem_axis_via_active_stems() {
    // M1: surgical via per-owner active stems.
    let mut store = EdgeStore::new();
    store.replace_parsed_edges(
        "/src/A.vue",
        BTreeSet::new(),
        vec![(
            ("./types".to_string(), ResolveRequestKind::EsmImport),
            "/src/types".to_string(),
        )],
        vec![],
    );
    store.replace_parsed_edges(
        "/src/B.vue",
        BTreeSet::new(),
        vec![(
            ("./types".to_string(), ResolveRequestKind::EsmImport),
            "/src/types".to_string(),
        )],
        vec![],
    );
    store.remove_file("/src/A.vue");
    assert_eq!(
        store.reverse_deps_for_target("/src/types", None),
        vec!["/src/B.vue"],
        "removed owner cleared from stem bucket; B remains"
    );
}

// ── Test #27 ──
#[test]
fn remove_file_keeps_the_importers_that_still_name_it() {
    // The reverse axis mirrors the importers' FORWARD state. Removing
    // /foo.ts as an OWNER (a delete, or the remove-then-reload of a disk
    // change) leaves /src/A.vue still naming it, so A must stay reachable
    // from it: an identical re-record of A is an idempotent no-op and could
    // never put the edge back.
    let mut store = EdgeStore::new();
    store.replace_parsed_edges("/src/A.vue", btree(&["/foo.ts"]), vec![], vec![]);
    store.replace_parsed_edges(
        "/src/B.vue",
        BTreeSet::new(),
        vec![(
            ("./foo".to_string(), ResolveRequestKind::EsmImport),
            "/foo".to_string(),
        )],
        vec![],
    );
    store.replace_parsed_edges("/foo.ts", btree(&["/dep.ts"]), vec![], vec![]);

    store.remove_file("/foo.ts");
    let mut importers = store.reverse_deps_for_target("/foo.ts", Some("/foo"));
    importers.sort();
    assert_eq!(importers, vec!["/src/A.vue", "/src/B.vue"]);
    assert!(
        store.reverse_deps_for_target("/dep.ts", None).is_empty(),
        "the removed owner's OWN forward edges are retracted"
    );

    // Reloading the file and re-recording the unchanged importer keeps it.
    store.replace_parsed_edges("/foo.ts", btree(&["/dep.ts"]), vec![], vec![]);
    store.replace_parsed_edges("/src/A.vue", btree(&["/foo.ts"]), vec![], vec![]);
    assert!(store
        .reverse_deps_for_target("/foo.ts", None)
        .contains(&"/src/A.vue".to_string()));

    // The bucket is retracted by the importer, the only party that owns it.
    store.replace_parsed_edges("/src/A.vue", BTreeSet::new(), vec![], vec![]);
    store.remove_file("/src/B.vue");
    assert!(store
        .reverse_deps_for_target("/foo.ts", Some("/foo"))
        .is_empty());
}

// ── Test #28 ──
#[test]
fn dependency_snapshot_view_returns_full_state() {
    let mut store = EdgeStore::new();
    store.replace_parsed_edges(
        "/src/Comp.vue",
        btree(&["/lib/p.ts"]),
        vec![(
            ("./u".to_string(), ResolveRequestKind::EsmImport),
            "/src/u".to_string(),
        )],
        vec![("vue".to_string(), ResolveRequestKind::EsmImport)],
    );
    store.add_lazy_resolved_dep("/src/Comp.vue", "/lib/lazy.ts");
    store.add_ambient_resolved_dep("/src/Comp.vue", "ambient:/A/x.d.ts");
    store.replace_semantic_transitive("/src/Comp.vue", btree(&["/lib/sem.ts"]));
    let snap = store.snapshot("/src/Comp.vue").expect("snapshot");
    assert!(snap.parsed_resolved.contains("/lib/p.ts"));
    assert!(snap
        .parsed_unresolved_relatives
        .contains_key(&("./u".to_string(), ResolveRequestKind::EsmImport)));
    assert!(snap.lazy_resolved.contains("/lib/lazy.ts"));
    assert!(snap.ambient_resolved.contains("ambient:/A/x.d.ts"));
    assert!(snap.semantic_transitive.contains("/lib/sem.ts"));
    assert_eq!(snap.bare_specifiers.len(), 1);
}

// ── Test #29 ──
#[test]
fn replace_exact_resolutions_with_none_target_does_not_dampen_stem() {
    // F18: resolved_canonical_id: None doesn't dampen; stem stays active.
    let mut store = EdgeStore::new();
    store.replace_parsed_edges(
        "/src/Comp.vue",
        BTreeSet::new(),
        vec![(
            ("./types".to_string(), ResolveRequestKind::EsmImport),
            "/src/types".to_string(),
        )],
        vec![],
    );
    store.replace_exact_resolutions(
        "/src/Comp.vue",
        vec![exact("./types", None, vec!["/lib/types.ts"])],
    );
    // Stem still active.
    assert_eq!(
        store.reverse_deps_for_target("/src/types", None),
        vec!["/src/Comp.vue"],
        "None resolved_canonical_id must NOT dampen the stem"
    );
}

// ── Test #30 ──
#[test]
fn replace_exact_resolutions_removed_resolution_restores_stem() {
    // F18: bundler removes resolution; previously-dampened stem becomes
    // active again.
    let mut store = EdgeStore::new();
    store.replace_parsed_edges(
        "/src/Comp.vue",
        BTreeSet::new(),
        vec![(
            ("./types".to_string(), ResolveRequestKind::EsmImport),
            "/src/types".to_string(),
        )],
        vec![],
    );
    store.replace_exact_resolutions(
        "/src/Comp.vue",
        vec![exact("./types", Some("/lib/types.ts"), vec![])],
    );
    assert!(
        store.reverse_deps_for_target("/src/types", None).is_empty(),
        "stem dampened first"
    );
    // Bundler removes the resolution (passes empty list).
    store.replace_exact_resolutions("/src/Comp.vue", vec![]);
    assert_eq!(
        store.reverse_deps_for_target("/src/types", None),
        vec!["/src/Comp.vue"],
        "stem RESTORED to active after bundler removes resolution"
    );
}

// ── Test #31 ──
#[test]
fn replace_exact_resolutions_changed_to_none_restores_stem() {
    // F18: bundler changes Some→None; stem reactivated.
    let mut store = EdgeStore::new();
    store.replace_parsed_edges(
        "/src/Comp.vue",
        BTreeSet::new(),
        vec![(
            ("./types".to_string(), ResolveRequestKind::EsmImport),
            "/src/types".to_string(),
        )],
        vec![],
    );
    store.replace_exact_resolutions(
        "/src/Comp.vue",
        vec![exact("./types", Some("/lib/types.ts"), vec![])],
    );
    // Bundler changes Some -> None for same specifier.
    store.replace_exact_resolutions("/src/Comp.vue", vec![exact("./types", None, vec![])]);
    assert_eq!(
        store.reverse_deps_for_target("/src/types", None),
        vec!["/src/Comp.vue"],
        "stem reactivated after Some→None change"
    );
}

// ── Test #32 ──
#[test]
fn record_parsed_edges_followed_by_set_exact_round_trip() {
    // Sequence: parse `./types` (stem present) → bundler resolves
    // (stem dampened, canonical present) → STRUCTURALLY DIFFERENT parse
    // re-record (per F11 lifecycle: clears exact_resolutions, so stem
    // becomes active again, canonical empty).
    //
    // R22 contract: the F11 lifecycle survives only on the
    // structural-change branch — a byte-identical re-record is a TRUE
    // no-op and would NOT clear `exact_resolutions`. This test
    // discriminates by introducing a SECOND unresolved relative on the
    // re-record (`./types-v2`), so `parsed_unresolved_relatives`
    // genuinely differs from the snapshot and the clear lifecycle
    // fires.
    let mut store = EdgeStore::new();
    store.replace_parsed_edges(
        "/src/Comp.vue",
        BTreeSet::new(),
        vec![(
            ("./types".to_string(), ResolveRequestKind::EsmImport),
            "/src/types".to_string(),
        )],
        vec![],
    );
    assert_eq!(
        store.reverse_deps_for_target("/src/types", None),
        vec!["/src/Comp.vue"],
    );
    store.replace_exact_resolutions(
        "/src/Comp.vue",
        vec![exact("./types", Some("/lib/types.ts"), vec![])],
    );
    assert!(store.reverse_deps_for_target("/src/types", None).is_empty());
    assert_eq!(
        store.reverse_deps_for_target("/lib/types.ts", None),
        vec!["/src/Comp.vue"],
    );
    // Structurally-different re-record (a second unresolved relative
    // makes the input set diverge from the snapshot): clears
    // exact_resolutions; stem becomes active again.
    store.replace_parsed_edges(
        "/src/Comp.vue",
        BTreeSet::new(),
        vec![
            (
                ("./types".to_string(), ResolveRequestKind::EsmImport),
                "/src/types".to_string(),
            ),
            (
                ("./types-v2".to_string(), ResolveRequestKind::EsmImport),
                "/src/types-v2".to_string(),
            ),
        ],
        vec![],
    );
    assert!(
        store
            .reverse_deps_for_target("/lib/types.ts", None)
            .is_empty(),
        "F11: exact_resolved cleared on structurally-changed re-record"
    );
    assert_eq!(
        store.reverse_deps_for_target("/src/types", None),
        vec!["/src/Comp.vue"],
        "stem reactivated after structurally-changed re-record \
         (exact_resolutions cleared)"
    );
}

// ── Test #34 (R5: replaces deleted #33) ──
#[test]
fn dampening_restricted_to_codegen_blocker_phase() {
    // R5: a ProviderGraph-only exact does NOT dampen a
    // parsed-unresolved CodegenBlocker stem.
    let mut store = EdgeStore::new();
    store.replace_parsed_edges(
        "/src/Comp.vue",
        BTreeSet::new(),
        vec![(
            ("./types".to_string(), ResolveRequestKind::EsmImport),
            "/src/types".to_string(),
        )],
        vec![],
    );
    // Single ProviderGraph exact — does NOT dampen.
    store.replace_exact_resolutions(
        "/src/Comp.vue",
        vec![exact_with(
            "./types",
            ResolvePhase::ProviderGraph,
            ResolveRequestKind::EsmImport,
            Some("/lib/types.ts"),
        )],
    );
    assert_eq!(
        store.reverse_deps_for_target("/src/types", None),
        vec!["/src/Comp.vue"],
        "ProviderGraph-only exact must NOT dampen a CodegenBlocker stem"
    );
    // Add CodegenBlocker exact alongside — now stem IS dampened.
    store.replace_exact_resolutions(
        "/src/Comp.vue",
        vec![
            exact_with(
                "./types",
                ResolvePhase::ProviderGraph,
                ResolveRequestKind::EsmImport,
                Some("/lib/types.ts"),
            ),
            exact_with(
                "./types",
                ResolvePhase::CodegenBlocker,
                ResolveRequestKind::EsmImport,
                Some("/lib/types.ts"),
            ),
        ],
    );
    assert!(
        store.reverse_deps_for_target("/src/types", None).is_empty(),
        "CodegenBlocker exact dampens the stem"
    );
}

// ── Backward-compat smoke tests (existing API names retained) ──

#[test]
fn exact_resolution_not_found_for_unknown_file() {
    let store = EdgeStore::new();
    assert!(store
        .get_exact_resolution("src/foo.vue", "./bar", default_ctx())
        .is_none());
    assert!(!store.has_exact_resolutions("src/foo.vue"));
}

#[test]
fn forward_deps_includes_all_classes() {
    let mut store = EdgeStore::new();
    store.replace_parsed_edges("/src/Comp.vue", btree(&["/lib/p.ts"]), vec![], vec![]);
    store.replace_exact_resolutions(
        "/src/Comp.vue",
        vec![exact("./e", Some("/lib/e.ts"), vec![])],
    );
    store.add_lazy_resolved_dep("/src/Comp.vue", "/lib/l.ts");
    store.add_ambient_resolved_dep("/src/Comp.vue", "ambient:/A/x.d.ts");
    store.replace_semantic_transitive("/src/Comp.vue", btree(&["/lib/s.ts"]));
    let mut got = store.forward_deps("/src/Comp.vue");
    got.sort();
    let mut want = vec![
        "/lib/p.ts",
        "/lib/e.ts",
        "/lib/l.ts",
        "ambient:/A/x.d.ts",
        "/lib/s.ts",
    ];
    want.sort();
    assert_eq!(got, want);
}

/// The `replace_exact_resolutions` no-op gate must be duplicate-key
/// safe: an input carrying the SAME key twice (`[A→x, A→x]`) against a
/// stored table `{A→x, B→y}` has matching lengths and every input entry
/// matches the stored value — but a real replace would DROP `B→y`. The
/// gate must count DISTINCT input keys, not raw input length, so the
/// drop is reported as a change and the caller's invalidation cascade
/// runs.
#[test]
fn replace_exact_resolutions_noop_gate_is_duplicate_key_safe() {
    let mut store = EdgeStore::new();
    store.replace_exact_resolutions(
        "/src/Comp.vue",
        vec![
            exact("./a", Some("/lib/a.ts"), vec![]),
            exact("./b", Some("/lib/b.ts"), vec![]),
        ],
    );

    // Duplicate-keyed input: same length as the stored table, every
    // entry value-matches its stored counterpart — but it names only
    // ONE distinct key, so the replace drops `./b`.
    let result = store.replace_exact_resolutions(
        "/src/Comp.vue",
        vec![
            exact("./a", Some("/lib/a.ts"), vec![]),
            exact("./a", Some("/lib/a.ts"), vec![]),
        ],
    );
    assert!(
        result.changed,
        "a duplicate-keyed input that drops a stored entry MUST report \
         changed — reporting a no-op skips the invalidation cascade and \
         leaves a stale exact resolution observable",
    );
    assert!(
        store
            .get_exact_resolution("/src/Comp.vue", "./b", default_ctx())
            .is_none(),
        "the replace must actually drop './b' (wholesale-replace semantics)",
    );

    // Control: a genuinely identical duplicate-keyed re-push against the
    // now single-entry table IS a value no-op (distinct keys == stored
    // keys, every value matches).
    let idempotent = store.replace_exact_resolutions(
        "/src/Comp.vue",
        vec![
            exact("./a", Some("/lib/a.ts"), vec![]),
            exact("./a", Some("/lib/a.ts"), vec![]),
        ],
    );
    assert!(
        !idempotent.changed,
        "a duplicate-keyed input whose distinct-key set value-matches the \
         stored table is a true no-op",
    );
}

#[test]
fn a_superseded_duplicate_route_stores_no_dependency_evidence_fresh_or_incremental() {
    let refresh = || {
        vec![
            exact("./a", Some("/lib/x.ts"), vec![]),
            exact("./a", Some("/lib/y.ts"), vec![]),
        ]
    };
    let mut fresh = EdgeStore::new();
    assert!(
        fresh
            .replace_exact_resolutions("/src/Comp.vue", refresh())
            .changed
    );

    let mut incremental = EdgeStore::new();
    incremental.replace_exact_resolutions(
        "/src/Comp.vue",
        vec![exact("./a", Some("/lib/y.ts"), vec![])],
    );
    let result = incremental.replace_exact_resolutions("/src/Comp.vue", refresh());
    assert!(
        !result.changed,
        "the last route per key already matches the stored table"
    );

    for (label, store) in [("fresh", &fresh), ("incremental", &incremental)] {
        let snapshot = store.snapshot("/src/Comp.vue").expect("owner state");
        assert_eq!(
            snapshot.exact_resolved,
            BTreeSet::from(["/lib/y.ts".to_string()]),
            "{label}: a superseded route's target is no exact dependency"
        );
        assert!(
            store.reverse_deps("/lib/x.ts").is_empty(),
            "{label}: no reverse edge may point from a superseded route's target"
        );
        assert_eq!(
            store.reverse_deps("/lib/y.ts"),
            vec!["/src/Comp.vue".to_string()],
            "{label}: the winning route's target keeps its reverse edge"
        );
    }
}

#[test]
fn an_empty_refresh_of_an_owner_without_edge_state_is_unchanged() {
    let mut store = EdgeStore::new();
    let result = store.replace_exact_resolutions("/src/never.ts", vec![]);
    assert!(
        !result.changed,
        "an owner with no edge state stores the empty table already"
    );
    assert!(
        store.snapshot("/src/never.ts").is_none(),
        "an unchanged refresh creates no owner state"
    );
}

// ── Owner-local exact publication ──

mod owner_local_publication {
    use std::collections::BTreeSet;
    use std::sync::{mpsc, Arc, Barrier};
    use std::time::Duration;

    use crate::changes::WorkspaceChange;
    use crate::engine::resolution_test_hooks::{self, ResolutionPhase};
    use crate::memory::{MemoryOptions, MemoryWorkspace};
    use crate::resolution_currency::{
        take_exact_publication_work, CapturedResolutionWorld, ResolutionFactKey,
        ResolutionFactVersion,
    };
    use crate::traits::{WorkspaceAccess, WorkspaceRead};
    use crate::types::{ExactResolution, ParsedEdge};
    use verter_session_query::resolution::{
        ResolutionContext, ResolutionPopulation, ResolvePhase, ResolveRequestKind,
    };

    const OWNER_COUNTS: [usize; 4] = [128, 256, 512, 1024];

    fn owner_id(index: usize) -> String {
        format!("/p/src/owner{index:04}.ts")
    }

    /// Two routes per owner, distinct per owner and per revision.
    fn owner_routes(index: usize, revision: usize) -> Vec<ExactResolution> {
        vec![
            ExactResolution {
                specifier: "./a".to_string(),
                phase: ResolvePhase::CodegenBlocker,
                kind: ResolveRequestKind::EsmImport,
                resolved_canonical_id: Some(format!("/p/dep/a{index}_{revision}.ts")),
                possible_canonical_ids: vec![format!("/p/dep/a{index}_{revision}.ts")],
            },
            ExactResolution {
                specifier: "./b".to_string(),
                phase: ResolvePhase::ProviderGraph,
                kind: ResolveRequestKind::EsmImport,
                resolved_canonical_id: Some(format!("/p/dep/b{index}_{revision}.ts")),
                possible_canonical_ids: vec![format!("/p/dep/b{index}_{revision}.ts")],
            },
        ]
    }

    fn route_fact_keys(index: usize) -> Vec<ResolutionFactKey> {
        owner_routes(index, 0)
            .iter()
            .map(|route| {
                ResolutionFactKey::exact_importer(
                    &owner_id(index),
                    &route.specifier,
                    ResolutionContext {
                        phase: route.phase,
                        kind: route.kind,
                    },
                    ResolutionPopulation::Base,
                )
            })
            .collect()
    }

    fn populated(owners: usize) -> MemoryWorkspace {
        let workspace = MemoryWorkspace::new(MemoryOptions::default());
        for index in 0..owners {
            workspace.set_exact_resolutions(&owner_id(index), owner_routes(index, 0));
        }
        workspace
    }

    /// Exact-table work of one refresh of `owner` in a workspace of `owners`
    /// owners: unchanged and changed exact refreshes, an unchanged and a
    /// changed parsed-edge refresh carrying the exact routes, and the
    /// owner's deletion.
    fn refresh_work(owners: usize, owner: usize) -> [u64; 5] {
        let workspace = populated(owners);
        let bare = [ParsedEdge::Bare {
            specifier: "pkg".to_string(),
            kind: ResolveRequestKind::EsmImport,
        }];
        workspace.record_parsed_edges_with_exact_resolutions(
            &owner_id(owner),
            &bare,
            owner_routes(owner, 0),
        );
        let _ = take_exact_publication_work();

        let unchanged = workspace.set_exact_resolutions(&owner_id(owner), owner_routes(owner, 0));
        assert!(!unchanged.changed, "an identical refresh must not publish");
        let unchanged_exact = take_exact_publication_work();

        let changed = workspace.set_exact_resolutions(&owner_id(owner), owner_routes(owner, 1));
        assert!(changed.changed, "a changed refresh must publish");
        let changed_exact = take_exact_publication_work();

        workspace.record_parsed_edges_with_exact_resolutions(
            &owner_id(owner),
            &bare,
            owner_routes(owner, 1),
        );
        let unchanged_parsed = take_exact_publication_work();

        workspace.record_parsed_edges_with_exact_resolutions(
            &owner_id(owner),
            &bare,
            owner_routes(owner, 2),
        );
        let changed_parsed = take_exact_publication_work();

        workspace.apply_changes(vec![WorkspaceChange::FileDeleted {
            canonical_id: owner_id(owner),
        }]);
        let deleted = take_exact_publication_work();

        [
            unchanged_exact,
            changed_exact,
            unchanged_parsed,
            changed_parsed,
            deleted,
        ]
    }

    #[test]
    fn exact_refresh_work_is_independent_of_unrelated_owner_count() {
        let samples: Vec<[u64; 5]> = OWNER_COUNTS
            .iter()
            .map(|&owners| refresh_work(owners, owners / 2))
            .collect();
        let labels = [
            "unchanged exact refresh",
            "changed exact refresh",
            "unchanged parsed-edge refresh",
            "changed parsed-edge refresh",
            "owner deletion",
        ];
        for (operation, label) in labels.iter().enumerate() {
            let work: Vec<u64> = samples.iter().map(|sample| sample[operation]).collect();
            assert!(
                work[0] > 0,
                "{label}: the work counter must observe the refresh at all"
            );
            assert!(
                work.iter().all(|&w| w == work[0]),
                "{label}: exact-table work must be owner-local, identical at \
                 {OWNER_COUNTS:?} owners with two routes each; observed {work:?}"
            );
        }
    }

    #[test]
    fn subtree_owner_seek_visits_only_the_subtree_key_range() {
        use crate::resolution_currency::ResolutionWorldRoot;
        use verter_session_query::resolution::ResolutionWorldId;

        // Unrelated owners elsewhere AND component-prefix siblings of the
        // directory (`/p/sub-…`, `/p/sub.…`, `/p/subway/…`, all adjacent to
        // `/p/sub/` in key order) grow with `owners`; the subtree's own
        // importers stay fixed.
        let seek = |owners: usize| {
            let mut root = ResolutionWorldRoot::bootstrap(ResolutionWorldId::from_raw(1));
            for index in 0..owners {
                root.replace_owner_exacts(&owner_id(index), &owner_routes(index, 0));
                for sibling in [
                    format!("/p/sub-{index:04}.ts"),
                    format!("/p/sub.{index:04}.ts"),
                    format!("/p/subway/{index:04}.ts"),
                ] {
                    root.replace_owner_exacts(&sibling, &owner_routes(index, 0));
                }
            }
            for (owner, index) in [("/p/sub", 0), ("/p/sub/a.ts", 1), ("/p/sub/b/c.ts", 2)] {
                root.replace_owner_exacts(owner, &owner_routes(index, 0));
            }
            let _ = take_exact_publication_work();
            let under = root.exact_owners_under("/p/sub/");
            (under, take_exact_publication_work())
        };
        let samples: Vec<_> = OWNER_COUNTS.iter().map(|&owners| seek(owners)).collect();
        for (owners, (under, _)) in OWNER_COUNTS.iter().zip(&samples) {
            assert_eq!(
                under,
                &[
                    "/p/sub".to_string(),
                    "/p/sub/a.ts".to_string(),
                    "/p/sub/b/c.ts".to_string()
                ],
                "{owners} owners: exactly the directory's own path and the importers under it"
            );
        }
        let work: Vec<u64> = samples.iter().map(|(_, work)| *work).collect();
        assert!(
            work.iter().all(|&w| w == work[0]),
            "the seek must not visit importers outside the subtree, component-prefix \
             siblings included; observed {work:?}"
        );
    }

    #[test]
    fn root_replacement_shares_every_unrelated_owner_bucket() {
        let owners = 256;
        let workspace = populated(owners);
        let engine = &workspace.engine;
        let before = engine
            .capture_published_resolution_world(ResolutionPopulation::Base)
            .expect("a settled world");
        let versions_before: Vec<_> = (0..owners)
            .flat_map(route_fact_keys)
            .map(|key| engine.resolution_fact_version_for_test(ResolutionPopulation::Base, &key))
            .collect();

        workspace.set_exact_resolutions(&owner_id(7), owner_routes(7, 1));

        let after = engine
            .capture_published_resolution_world(ResolutionPopulation::Base)
            .expect("a settled world");
        for index in (0..owners).filter(|&index| index != 7) {
            let id = owner_id(index);
            assert!(
                Arc::ptr_eq(
                    &before.base.exact_bucket(&id).expect("populated owner"),
                    &after.base.exact_bucket(&id).expect("populated owner"),
                ),
                "root replacement must share {id}'s untouched bucket"
            );
        }
        assert!(
            !Arc::ptr_eq(
                &before.base.exact_bucket(&owner_id(7)).expect("populated"),
                &after.base.exact_bucket(&owner_id(7)).expect("populated"),
            ),
            "the refreshed owner publishes a replacement bucket"
        );
        assert_eq!(
            before
                .base
                .exact(&owner_id(7), "./a", context(ResolvePhase::CodegenBlocker)),
            Some(&owner_routes(7, 0)[0]),
            "a held root keeps the bucket it was published with"
        );
        let versions_after: Vec<_> = (0..owners)
            .flat_map(route_fact_keys)
            .map(|key| engine.resolution_fact_version_for_test(ResolutionPopulation::Base, &key))
            .collect();
        for (index, (was, now)) in versions_before.iter().zip(&versions_after).enumerate() {
            let owner = index / 2;
            if owner == 7 {
                assert_ne!(was, now, "the refreshed owner's route facts advance");
            } else {
                assert_eq!(
                    was, now,
                    "owner {owner}'s route fact must keep its identity across another \
                     owner's refresh"
                );
            }
        }
    }

    /// Owners populated through real resolutions, so the root's recorded
    /// probes and realpaths grow with the owner count. An unchanged exact
    /// refresh opens no publication window and leaves the published root in
    /// place; a changed one builds its replacement without copying any of
    /// those unrelated observations.
    #[test]
    fn exact_refresh_copies_no_unrelated_root_observation() {
        use std::cell::Cell;
        use std::rc::Rc;

        let owners = 256;
        let refreshed = 9;
        let workspace = MemoryWorkspace::new(MemoryOptions::default());
        for index in 0..owners {
            workspace.inject_file(owner_id(index), Arc::from("export {}\n"));
            workspace.inject_file(format!("/p/src/dep{index:04}.ts"), Arc::from("export {}\n"));
            assert!(
                workspace
                    .resolve_import(
                        &owner_id(index),
                        &format!("./dep{index:04}"),
                        context(ResolvePhase::CodegenBlocker),
                    )
                    .is_some(),
                "owner {index} resolves its sibling"
            );
            workspace.set_exact_resolutions(&owner_id(index), owner_routes(index, 0));
        }
        let engine = &workspace.engine;
        let before = engine
            .capture_published_resolution_world(ResolutionPopulation::Base)
            .expect("a settled world");
        assert!(
            before.base.path_probes.len() >= owners,
            "the fixture's resolutions record probes for every owner"
        );

        let windows = Rc::new(Cell::new(0));
        let unchanged = resolution_test_hooks::with_repeating_hook(
            ResolutionPhase::WorldWriteHeld,
            {
                let windows = Rc::clone(&windows);
                move || windows.set(windows.get() + 1)
            },
            || workspace.set_exact_resolutions(&owner_id(refreshed), owner_routes(refreshed, 0)),
        );
        assert!(!unchanged.changed, "an identical refresh must not publish");
        assert_eq!(
            windows.get(),
            0,
            "an unchanged refresh must not open a publication window"
        );
        let after_unchanged = engine
            .capture_published_resolution_world(ResolutionPopulation::Base)
            .expect("a settled world");
        assert!(
            Arc::ptr_eq(&before.base, &after_unchanged.base),
            "an unchanged refresh leaves the published root in place"
        );

        let changed =
            workspace.set_exact_resolutions(&owner_id(refreshed), owner_routes(refreshed, 1));
        assert!(changed.changed, "a changed refresh must publish");
        let after_changed = engine
            .capture_published_resolution_world(ResolutionPopulation::Base)
            .expect("a settled world");
        assert!(
            !Arc::ptr_eq(&before.base, &after_changed.base),
            "a changed refresh publishes a replacement root"
        );
        assert!(
            after_changed
                .base
                .shares_observation_maps_with(&before.base),
            "the replacement root shares every unrelated recorded observation"
        );
    }

    #[test]
    fn a_refresh_whose_last_route_per_key_matches_the_published_bucket_is_unchanged() {
        let workspace = populated(4);
        let published = owner_routes(2, 0);
        let mut superseded = owner_routes(2, 1);
        superseded.extend(published.iter().cloned());

        let result = workspace.set_exact_resolutions(&owner_id(2), superseded);

        assert!(
            !result.changed,
            "the last route per key wins, so this refresh republishes the stored bucket \
             and the edge store and the world must both report it unchanged"
        );
        assert_eq!(
            published_routes(&workspace, 4)[2],
            (Some(published[0].clone()), Some(published[1].clone()))
        );
    }

    fn context(phase: ResolvePhase) -> ResolutionContext {
        ResolutionContext {
            phase,
            kind: ResolveRequestKind::EsmImport,
        }
    }

    /// Every owner's published routes, read from the current root.
    fn published_routes(
        workspace: &MemoryWorkspace,
        owners: usize,
    ) -> Vec<(Option<ExactResolution>, Option<ExactResolution>)> {
        let world = workspace
            .engine
            .capture_published_resolution_world(ResolutionPopulation::Base)
            .expect("a settled world");
        (0..owners)
            .map(|index| {
                let id = owner_id(index);
                (
                    world
                        .base
                        .exact(&id, "./a", context(ResolvePhase::CodegenBlocker))
                        .cloned(),
                    world
                        .base
                        .exact(&id, "./b", context(ResolvePhase::ProviderGraph))
                        .cloned(),
                )
            })
            .collect()
    }

    #[test]
    fn an_empty_refresh_of_an_importer_the_workspace_never_saw_publishes_nothing() {
        let workspace = populated(4);
        let before = workspace
            .engine
            .capture_published_resolution_world(ResolutionPopulation::Base)
            .expect("a settled world");

        let result = workspace.set_exact_resolutions("/p/never.ts", vec![]);

        assert!(!result.changed, "no route existed and none is published");
        let after = workspace
            .engine
            .capture_published_resolution_world(ResolutionPopulation::Base)
            .expect("a settled world");
        assert!(
            Arc::ptr_eq(&before.base, &after.base),
            "an unchanged refresh publishes no replacement root"
        );
    }

    /// Bounds every channel wait so a broken interleaving fails instead of
    /// hanging; no correct run comes near it.
    const WAIT: Duration = Duration::from_secs(30);

    fn route_versions(world: &CapturedResolutionWorld, index: usize) -> Vec<ResolutionFactVersion> {
        route_fact_keys(index)
            .iter()
            .map(|key| world.fact_version(key))
            .collect()
    }

    /// Edge-store exact evidence of `owner`: its exact dependencies and the
    /// importers of each target it ever routed to.
    fn edge_evidence(
        workspace: &MemoryWorkspace,
        index: usize,
        revisions: usize,
    ) -> (BTreeSet<String>, Vec<Vec<String>>) {
        let edges = workspace.engine.edges.read();
        let exact_resolved = edges
            .snapshot(&owner_id(index))
            .map(|snapshot| snapshot.exact_resolved)
            .unwrap_or_default();
        let reverse = (0..revisions)
            .flat_map(|revision| owner_routes(index, revision))
            .filter_map(|route| route.resolved_canonical_id)
            .map(|target| edges.reverse_deps(&target))
            .collect();
        (exact_resolved, reverse)
    }

    /// One changed exact refresh is held inside its publication window while
    /// a competing parsed-edge publication and a resolution reach the
    /// publication gate it holds. Nothing observes the held publication
    /// partially; both queued operations run against the world it leaves;
    /// both owners' updates survive; every other owner's bucket and route
    /// facts keep their identity; and the result equals a fresh build.
    #[test]
    fn a_held_exact_publication_lands_whole_before_the_publications_queued_behind_it() {
        let owners = 128;
        let held = 3;
        let queued = 5;
        let workspace = Arc::new(populated(owners));
        let held_target = owner_routes(held, 1)[0]
            .resolved_canonical_id
            .clone()
            .expect("routed");
        workspace.inject_file(held_target.clone(), Arc::from("export {}\n"));
        let before = workspace
            .engine
            .capture_published_resolution_world(ResolutionPopulation::Base)
            .expect("a settled world");

        let (held_tx, held_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel::<()>();
        let held_writer = {
            let workspace = Arc::clone(&workspace);
            std::thread::spawn(move || {
                resolution_test_hooks::with_hook(
                    ResolutionPhase::WorldWriteHeld,
                    move || {
                        held_tx.send(()).expect("the test is listening");
                        release_rx
                            .recv_timeout(WAIT)
                            .expect("the test releases the held publication");
                    },
                    || workspace.set_exact_resolutions(&owner_id(held), owner_routes(held, 1)),
                )
            })
        };
        held_rx
            .recv_timeout(WAIT)
            .expect("the exact refresh enters its publication window");
        assert!(
            workspace
                .engine
                .capture_published_resolution_world(ResolutionPopulation::Base)
                .is_none(),
            "no capture may observe a root while its exact publication is in flight"
        );

        let (queued_tx, queued_rx) = mpsc::channel();
        let queued_writer = {
            let workspace = Arc::clone(&workspace);
            std::thread::spawn(move || {
                resolution_test_hooks::with_every_phase_hook(
                    move |phase| {
                        let _ = queued_tx.send(phase);
                    },
                    || {
                        workspace.record_parsed_edges_with_exact_resolutions(
                            &owner_id(queued),
                            &[],
                            owner_routes(queued, 1),
                        )
                    },
                )
            })
        };
        loop {
            let phase = queued_rx
                .recv_timeout(WAIT)
                .expect("the parsed-edge publication reaches the publication gate");
            assert_ne!(
                phase,
                ResolutionPhase::WorldWriteHeld,
                "a competing publication must not enter its window while another is held"
            );
            if phase == ResolutionPhase::PublicationGateWait {
                break;
            }
        }

        let (reader_tx, reader_rx) = mpsc::channel();
        let reader = {
            let workspace = Arc::clone(&workspace);
            std::thread::spawn(move || {
                resolution_test_hooks::with_hook(
                    ResolutionPhase::PublicationGateWait,
                    move || reader_tx.send(()).expect("the test is listening"),
                    || {
                        workspace.resolve_import(
                            &owner_id(held),
                            "./a",
                            context(ResolvePhase::CodegenBlocker),
                        )
                    },
                )
            })
        };
        reader_rx
            .recv_timeout(WAIT)
            .expect("the resolution waits on the held publication");

        release_tx
            .send(())
            .expect("the held publication is waiting");
        assert!(
            held_writer.join().expect("held writer").changed,
            "the held refresh publishes"
        );
        assert!(
            queued_writer.join().expect("queued writer").changed,
            "the queued refresh publishes"
        );
        assert_eq!(
            reader
                .join()
                .expect("reader")
                .map(|resolved| resolved.source_id),
            Some(held_target),
            "a resolution that waited on the publication answers from the world it left"
        );

        let after = workspace
            .engine
            .capture_published_resolution_world(ResolutionPopulation::Base)
            .expect("a settled world");
        for index in [held, queued] {
            let id = owner_id(index);
            assert_eq!(
                before
                    .base
                    .exact(&id, "./a", context(ResolvePhase::CodegenBlocker)),
                Some(&owner_routes(index, 0)[0]),
                "the root held from before keeps {id}'s old routes"
            );
            assert_eq!(
                after
                    .base
                    .exact(&id, "./a", context(ResolvePhase::CodegenBlocker)),
                Some(&owner_routes(index, 1)[0]),
                "{id}'s update survives the other publication"
            );
            assert!(
                route_versions(&after, index)
                    .iter()
                    .zip(route_versions(&before, index))
                    .all(|(now, was)| *now > was),
                "{id}'s route facts advance with its routes"
            );
        }
        for index in (0..owners).filter(|index| ![held, queued].contains(index)) {
            let id = owner_id(index);
            assert!(
                Arc::ptr_eq(
                    &before.base.exact_bucket(&id).expect("populated owner"),
                    &after.base.exact_bucket(&id).expect("populated owner"),
                ),
                "neither publication may replace {id}'s bucket"
            );
            assert_eq!(
                route_versions(&before, index),
                route_versions(&after, index),
                "{id}'s route facts keep their identity"
            );
        }

        let fresh = MemoryWorkspace::new(MemoryOptions::default());
        for index in 0..owners {
            let revision = usize::from([held, queued].contains(&index));
            fresh.set_exact_resolutions(&owner_id(index), owner_routes(index, revision));
        }
        assert_eq!(
            published_routes(&workspace, owners),
            published_routes(&fresh, owners),
            "the interleaved publications publish exactly the table a fresh build does"
        );
        for index in [held, queued, 0] {
            assert_eq!(
                edge_evidence(&workspace, index, 2),
                edge_evidence(&fresh, index, 2),
                "owner {index}'s exact dependency evidence matches a fresh build"
            );
        }
    }

    #[test]
    fn concurrent_owner_refreshes_preserve_unrelated_buckets_and_match_a_fresh_build() {
        const WRITERS: usize = 4;
        const ROUNDS: usize = 24;
        let owners = 128;
        let workspace = Arc::new(populated(owners));
        let bystander = owner_id(owners - 1);
        let bystander_bucket = workspace
            .engine
            .capture_published_resolution_world(ResolutionPopulation::Base)
            .expect("a settled world")
            .base
            .exact_bucket(&bystander)
            .expect("populated owner");
        let barrier = Arc::new(Barrier::new(WRITERS + 1));

        let writers: Vec<_> = (0..WRITERS)
            .map(|writer| {
                let workspace = Arc::clone(&workspace);
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    for round in 0..ROUNDS {
                        // Each writer owns a disjoint stripe; every round
                        // republishes its stripe (alternating changed and
                        // unchanged refreshes through both writers).
                        for index in (writer..owners - 1).step_by(WRITERS) {
                            let revision = round / 2;
                            if round % 3 == 0 {
                                workspace.record_parsed_edges_with_exact_resolutions(
                                    &owner_id(index),
                                    &[],
                                    owner_routes(index, revision),
                                );
                            } else {
                                workspace.set_exact_resolutions(
                                    &owner_id(index),
                                    owner_routes(index, revision),
                                );
                            }
                        }
                    }
                })
            })
            .collect();
        let reader = {
            let workspace = Arc::clone(&workspace);
            let barrier = Arc::clone(&barrier);
            let bystander_bucket = Arc::clone(&bystander_bucket);
            std::thread::spawn(move || {
                barrier.wait();
                let mut observed = 0;
                while observed < ROUNDS * 8 {
                    // A capture fails only while a writer is mid-publication.
                    let Some(world) = workspace
                        .engine
                        .capture_published_resolution_world(ResolutionPopulation::Base)
                    else {
                        std::thread::yield_now();
                        continue;
                    };
                    observed += 1;
                    assert!(
                        Arc::ptr_eq(
                            &world
                                .base
                                .exact_bucket(&owner_id(owners - 1))
                                .expect("kept"),
                            &bystander_bucket,
                        ),
                        "no concurrent publication may replace an unrelated owner's bucket"
                    );
                    std::thread::yield_now();
                }
            })
        };
        for writer in writers {
            writer.join().expect("writer");
        }
        reader.join().expect("reader");

        let fresh = MemoryWorkspace::new(MemoryOptions::default());
        let final_revision = (ROUNDS - 1) / 2;
        for index in 0..owners {
            let revision = if index == owners - 1 {
                0
            } else {
                final_revision
            };
            fresh.set_exact_resolutions(&owner_id(index), owner_routes(index, revision));
        }
        assert_eq!(
            published_routes(&workspace, owners),
            published_routes(&fresh, owners),
            "incremental owner refreshes must publish exactly the table a fresh build does"
        );
        for index in 0..owners {
            assert_eq!(
                edge_evidence(&workspace, index, final_revision + 1),
                edge_evidence(&fresh, index, final_revision + 1),
                "owner {index}'s exact dependency evidence matches a fresh build"
            );
        }
    }
}
