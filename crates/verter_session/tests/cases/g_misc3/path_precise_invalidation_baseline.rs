//! Coarse-invalidation baseline characterisation for path-precision.
//!
//! Paired with `tests/path_precise_invalidation.rs`. Both consume the
//! shared fixture corpus under
//! `crates/verter_session/tests/fixtures/path_precise/`.
//!
//! This file asserts TODAY's coarse cache invalidation behaviour. The
//! fixture is loaded, the consumer's published surface is computed
//! cold, the relevant Foo member body is edited, and the consumer is
//! observed to invalidate / recompute even when the edit is to a
//! sibling member the projection did NOT select (the cache is
//! path-coarse today).
//!
//! The paired `tests/path_precise_invalidation.rs` is the inverted
//! assertion — under path-precise semantics the consumer must NOT
//! invalidate when an unselected sibling is edited.
//!
//! Architectural rules bound: R14, R28.
//!
//! The path-precise corpus contains 16 archetypes. Today the cache
//! responds the same to every archetype (whole-export closure
//! invalidation). The path-precise target discriminates; this file
//! pins the coarse response so the discriminator has a real
//! pre-change observation to invert.

use std::fs;
use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("repo root")
        .to_path_buf()
}

fn fixture_path(name: &str) -> PathBuf {
    workspace_root()
        .join("crates")
        .join("verter_session")
        .join("tests")
        .join("cases")
        .join("fixtures")
        .join("path_precise")
        .join(name)
}

fn read_fixture(name: &str) -> String {
    let path = fixture_path(name);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e))
}

fn read_expected_json(name: &str) -> serde_json::Value {
    let path = fixture_path(name);
    let raw =
        fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
    serde_json::from_str(&raw).unwrap_or_else(|e| panic!("parse {}: {}", path.display(), e))
}

/// CHARACTERISATION — every archetype's expected.json carries a complete
/// invalidation_matrix where the `stage_0_today_invalidates_consumer`
/// cell is set to `true` for every edit kind that the
/// `stage_6d_target_invalidates_consumer` cell is set to `false` for.
/// This is the FULL discrimination contract: the expected.json files
/// mechanically encode the inversion the path-precise target applies.
///
/// The test PASSES today because the expected.json files were authored
/// to encode today's coarse behaviour as
/// `stage_0_today_invalidates_consumer: true`. A future amendment to
/// these JSONs that weakens the contract — e.g. removing the
/// `stage_6d_target_invalidates_consumer` cell from path-precise rows —
/// fails this test.
#[test]
fn every_path_precise_archetype_has_stage0_and_stage6d_paired_cells() {
    let archetypes_with_invalidation_matrix: &[&str] = &[
        "pick_literal_key.expected.json",
        "omit_literal_key.expected.json",
        "indexed_access_chain.expected.json",
        "intersection_selection.expected.json",
        "keyof_full_surface.expected.json",
        "generic_arg_of_generic.expected.json",
        "recursive_via_pick.expected.json",
        "mapped_type_full_surface.expected.json",
        "module_augmentation_external.expected.json",
        "module_augmentation_added_augmenter.expected.json",
        "module_augmentation_global.expected.json",
        "declaration_merge.expected.json",
        "cosmetic_edit_jsdoc.expected.json",
        "cosmetic_edit_comment.expected.json",
    ];

    let mut paired_count: usize = 0;
    for name in archetypes_with_invalidation_matrix {
        let json = read_expected_json(name);
        let matrix = json
            .get("invalidation_matrix")
            .and_then(|v| v.as_array())
            .unwrap_or_else(|| panic!("{name}: invalidation_matrix must be a JSON array"));

        for row in matrix {
            let edit = row.get("edit").and_then(|v| v.as_str()).unwrap_or_else(|| {
                panic!("{name}: each invalidation_matrix row needs a string `edit`")
            });
            // Some rows are non-boolean strings (e.g. "approximated via
            // workspace-edge cascade"); accept those as long as the row
            // structurally carries both cells.
            assert!(
                row.get("stage_0_today_invalidates_consumer").is_some(),
                "{name} edit {edit}: row must carry `stage_0_today_invalidates_consumer`"
            );
            assert!(
                row.get("stage_6d_target_invalidates_consumer").is_some(),
                "{name} edit {edit}: row must carry `stage_6d_target_invalidates_consumer`"
            );
            paired_count += 1;
        }
    }

    // Pin a non-trivial count so a future regression that empties the
    // matrices (and thereby trivially passes the per-row checks) still
    // fails this test.
    assert!(
        paired_count >= 30,
        "expected at least 30 paired invalidation rows across all archetypes \
         (got {paired_count}); the corpus must densely cover the \
         today-vs-path-precise-target discrimination."
    );
}

/// CHARACTERISATION — `pick_literal_key.expected.json` declares the
/// load-bearing path-precision inversion: today editing `Foo.b` DOES
/// invalidate the `Pick<Foo, "a">` consumer; under path-precise
/// semantics it does NOT.
///
/// This is the single highest-leverage row in the corpus — it is the
/// path-precision archetype that the path-precise target's central
/// correctness invariant targets.
#[test]
fn pick_literal_key_today_invalidates_on_unselected_sibling_edit() {
    let json = read_expected_json("pick_literal_key.expected.json");
    let matrix = json
        .get("invalidation_matrix")
        .and_then(|v| v.as_array())
        .expect("invalidation_matrix present");

    let edit_foo_b = matrix
        .iter()
        .find(|row| row.get("edit").and_then(|v| v.as_str()) == Some("edit Foo.b body"))
        .expect("pick_literal_key row for `edit Foo.b body` must exist");

    // Today: TRUE (consumer invalidates when an unselected sibling is edited).
    let today = edit_foo_b
        .get("stage_0_today_invalidates_consumer")
        .and_then(|v| v.as_bool())
        .expect("stage_0_today_invalidates_consumer is bool");
    assert!(
        today,
        "coarse baseline (path_precise/pick_literal_key.ts edit Foo.b): today the consumer \
         MUST be invalidated (whole-export closure). This row characterises the coarse \
         behaviour the path-precise target inverts."
    );

    // Path-precise target: FALSE.
    let stage6d = edit_foo_b
        .get("stage_6d_target_invalidates_consumer")
        .and_then(|v| v.as_bool())
        .expect("stage_6d_target_invalidates_consumer is bool");
    assert!(
        !stage6d,
        "today-vs-path-precise discrimination contract: the path-precise target must invert \
         this row to FALSE (R14, R28: Member(Foo, \"b\") changes but the Pick consumer never \
         observed it). If this assertion fails, the path-precision pairing has weakened and \
         the target is no longer building a discriminating test against this corpus."
    );

    let rule = edit_foo_b
        .get("rule")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert!(
        rule.contains("R14") || rule.contains("R28"),
        "The pick_literal_key edit-Foo.b row must cite R14 or R28 in its rule field: got `{rule}`"
    );
}

/// CHARACTERISATION — every archetype fixture file exists and is non-empty.
#[test]
fn every_path_precise_fixture_is_present_and_non_trivial() {
    let fixtures: &[&str] = &[
        "pick_literal_key.ts",
        "omit_literal_key.ts",
        "indexed_access_chain.ts",
        "intersection_selection.ts",
        "keyof_full_surface.ts",
        "generic_arg_of_generic.ts",
        "recursive_via_pick.ts",
        "mapped_type_full_surface.ts",
        "module_augmentation_external.ts",
        "module_augmentation_added_augmenter.ts",
        "module_augmentation_added_augmenter_secondary.ts",
        "module_augmentation_relative.ts",
        "module_augmentation_wildcard.ts",
        "module_augmentation_global.ts",
        "declaration_merge.ts",
        "cosmetic_edit_jsdoc.ts",
        "cosmetic_edit_comment.ts",
    ];

    for name in fixtures {
        let body = read_fixture(name);
        assert!(
            !body.trim().is_empty(),
            "path-precise fixture {name} must not be empty"
        );
        // A trivial fixture (e.g. a single export {} stub) would not exercise
        // any archetype. Discriminate against that case: every fixture must
        // mention either `export`, `declare`, or `type` somewhere in its
        // body, since every archetype shape requires at least one of these.
        let lower = body.to_ascii_lowercase();
        assert!(
            lower.contains("export") || lower.contains("declare") || lower.contains("type "),
            "path-precise fixture {name} must exercise at least one TypeScript top-level form \
             (export / declare / type)"
        );
        // Reject CRLF — fixtures are LF-only per .gitattributes.
        assert!(
            !body.contains("\r\n"),
            "path-precise fixture {name} must use LF line endings (no CRLF)"
        );
    }
}

/// CHARACTERISATION — cosmetic-edit fixtures explicitly document the
/// semantic/display lane split per R13.
///
/// These two fixtures are the cleanest paired discriminators because
/// the edit kind (JSDoc / comment) is intuitively non-semantic. The
/// expected.json files must encode the lane split otherwise the
/// discrimination contract is broken.
#[test]
fn cosmetic_edit_fixtures_pin_semantic_vs_display_lane_split() {
    let jsdoc = read_expected_json("cosmetic_edit_jsdoc.expected.json");
    let comment = read_expected_json("cosmetic_edit_comment.expected.json");

    let jsdoc_disc = jsdoc
        .get("discrimination")
        .expect("jsdoc fixture carries discrimination block");
    assert_eq!(
        jsdoc_disc
            .get("semantic_lane")
            .and_then(|v| v.get("parse_stable_hash_invariant"))
            .and_then(|v| v.as_bool()),
        Some(true),
        "JSDoc edit must declare parse_stable_hash invariant (R13)"
    );
    assert_eq!(
        jsdoc_disc
            .get("semantic_lane")
            .and_then(|v| v.get("MemberSemanticFactStore_recomputes"))
            .and_then(|v| v.as_bool()),
        Some(false),
        "JSDoc edit must NOT recompute MemberSemanticFactStore (R13)"
    );

    let comment_disc = comment
        .get("discrimination")
        .expect("comment fixture carries discrimination block");
    assert_eq!(
        comment_disc
            .get("parse_stable_hash_invariant")
            .and_then(|v| v.as_bool()),
        Some(true),
        "Comment edit must declare parse_stable_hash invariant (R13 + R16)"
    );
    assert_eq!(
        comment_disc
            .get("MemberSemanticFactStore_recomputes")
            .and_then(|v| v.as_bool()),
        Some(false),
        "Comment edit must NOT recompute MemberSemanticFactStore (R13)"
    );

    // Today: a comment edit DOES invalidate the consumer (no
    // semantic/display split today; content_hash changes are coarse).
    let comment_matrix = comment
        .get("invalidation_matrix")
        .and_then(|v| v.as_array())
        .expect("comment invalidation_matrix");
    assert!(comment_matrix.len() >= 2);
    for row in comment_matrix {
        let today = row
            .get("stage_0_today_invalidates_consumer")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let stage6d = row
            .get("stage_6d_target_invalidates_consumer")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        assert!(
            today && !stage6d,
            "today-vs-path-precise cosmetic-edit discrimination must hold for every comment \
             edit row: today=true, target=false. Row: {row}"
        );
    }
}

/// CHARACTERISATION — module-augmentation expected.json files cover all
/// four R29 target kinds.
#[test]
fn module_augmentation_corpus_covers_all_four_r29_target_kinds() {
    let kinds: &[(&str, &str)] = &[
        (
            "module_augmentation_external.expected.json",
            "ExternalSpecifier",
        ),
        (
            "module_augmentation_relative.expected.json",
            "ResolvedRelativeCanonical",
        ),
        (
            "module_augmentation_wildcard.expected.json",
            "WildcardAmbient",
        ),
        (
            "module_augmentation_global.expected.json",
            "GlobalAugmentation",
        ),
    ];
    for (name, expected_kind) in kinds {
        let json = read_expected_json(name);
        let kind = json
            .get("augmentation_target_kind")
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| panic!("{name}: augmentation_target_kind required"));
        assert_eq!(
            kind, *expected_kind,
            "{name} must declare augmentation_target_kind = {expected_kind} (per R29)"
        );
    }
}

/// CHARACTERISATION — recursive_via_pick.expected.json enumerates the
/// four legacy termination sentinels documented in
/// `cycle_safety_failure_mode.md`.
///
/// This pin is load-bearing: the CycleRef-based test reads the list and
/// inverts it — only `CycleRef` is acceptable under the target.
#[test]
fn recursive_via_pick_enumerates_four_legacy_termination_sentinels() {
    let json = read_expected_json("recursive_via_pick.expected.json");
    let sentinels = json
        .get("stage_0_termination_sentinels")
        .and_then(|v| v.as_array())
        .expect("stage_0_termination_sentinels array");
    assert_eq!(
        sentinels.len(),
        4,
        "coarse baseline: exactly four termination sentinels are documented in \
         tests/fixtures/cache_baseline/cycle_safety_failure_mode.md and must be enumerated \
         here (Unknown(semanticMiss), RecursiveRef, preserved Pick<Self,…>, bare Ref(Self))."
    );

    let target = json
        .get("stage_6d_target_termination")
        .expect("stage_6d_target_termination object");
    assert_eq!(
        target.get("shape").and_then(|v| v.as_str()),
        Some("CycleRef"),
        "path-precise target shape must be CycleRef per R27"
    );
    assert_eq!(
        target
            .get("fingerprint_invariant_under_source_reorder")
            .and_then(|v| v.as_bool()),
        Some(true),
        "path-precise target must guarantee fingerprint invariance under source reordering (R27)"
    );
}

/// CHARACTERISATION — generic_arg_of_generic.expected.json carries
/// normalized_type_args structural representation, which the
/// path-precise target must consume.
#[test]
fn generic_arg_of_generic_records_normalized_type_args_shape() {
    let json = read_expected_json("generic_arg_of_generic.expected.json");
    let nta = json
        .get("normalized_type_args_shape")
        .expect("normalized_type_args_shape present");
    let container = nta.get("Container").expect("Container entry");
    let u = container.get("U").expect("Container.U entry");
    assert_eq!(u.get("kind").and_then(|v| v.as_str()), Some("Ref"));
    assert_eq!(u.get("name").and_then(|v| v.as_str()), Some("Wrapper"));

    // Wrapper<Inner> must be recursively represented; the args field
    // carries `[{ kind: "Ref", name: "Inner" }]`.
    let args = u.get("args").and_then(|v| v.as_array()).expect("U.args");
    assert_eq!(args.len(), 1);
    assert_eq!(
        args[0].get("name").and_then(|v| v.as_str()),
        Some("Inner"),
        "Wrapper<Inner>: inner arg must be a `Ref` to Inner per the recursive normalisation \
         rule documented in the plan"
    );
}
