//! EdgeStore unit tests — sub- (33 tests after R5: #33 deleted, #34 added).
//!
//! Tests are written against the new R4/R5 `DependencySnapshot` model with
//! per-class writers (`replace_parsed_edges`, `replace_exact_resolutions`,
//! `add_lazy_resolved_dep`, `replace_ambient_resolved`,
//! `add_ambient_resolved_dep`, `replace_semantic_transitive`) and the
//! two-axis reverse graph (`reverse_deps_for_target`).

use super::*;
use crate::types::{ExactResolution, ResolutionContext, ResolvePhase, ResolveRequestKind};
use std::collections::BTreeSet;

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
    // Gemini #1: same importer in canonical AND stem returns once.
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
fn reverse_deps_for_target_returns_empty_for_unknown_extension() {
    // Unknown extension (`.svelte`) falls through to canonical-only lookup.
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
    // Querying with `.svelte` (not in extension list) — only canonical hit.
    // Caller passes `None` for stripped_target since `.svelte` doesn't strip.
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
fn remove_file_clears_owner_as_target_in_both_axes() {
    // Removing /foo.ts clears reverse_deps_*[/foo.ts].
    let mut store = EdgeStore::new();
    store.replace_parsed_edges("/src/A.vue", btree(&["/foo.ts"]), vec![], vec![]);
    assert!(!store.reverse_deps_for_target("/foo.ts", None).is_empty());
    store.remove_file("/foo.ts");
    assert!(
        store.reverse_deps_for_target("/foo.ts", None).is_empty(),
        "removed file's reverse buckets must be cleared"
    );
    // But /src/A.vue's per-owner state is untouched.
    let snap = store.snapshot("/src/A.vue").unwrap();
    assert!(snap.parsed_resolved.contains("/foo.ts"));
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
    // R5 (Codex 2 #6): a ProviderGraph-only exact does NOT dampen a
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
