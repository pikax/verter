use super::*;

fn write(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, content).unwrap();
}

// ── pure helpers ──────────────────────────────────────────────────────

#[test]
fn source_map_identity_is_stable_and_profile_sensitive() {
    let a = compute_source_map_identity("p1", "MAP");
    let b = compute_source_map_identity("p1", "MAP");
    let c = compute_source_map_identity("p2", "MAP");
    let d = compute_source_map_identity("p1", "MAP2");
    assert_eq!(a, b, "same inputs => same identity");
    assert_ne!(a, c, "different profile => different identity");
    assert_ne!(a, d, "different map content => different identity");
}

#[test]
fn rewrite_vue_imports_is_idempotent_and_covers_both_quotes() {
    assert_eq!(
        rewrite_vue_imports("import X from './X.vue'"),
        "import X from './X.vue.ts'"
    );
    assert_eq!(
        rewrite_vue_imports("import X from \"./X.vue\""),
        "import X from \"./X.vue.ts\""
    );
    // Idempotent — an already-rewritten specifier is untouched.
    let once = rewrite_vue_imports("import X from './X.vue'");
    assert_eq!(rewrite_vue_imports(&once), once);
}

#[test]
fn map_absent_is_recorded_not_crashed() {
    // A present map yields a stable identity.
    match classify_source_map("p", Some("MAP")) {
        MapOutcome::Identity(id) => assert_eq!(id, compute_source_map_identity("p", "MAP")),
        MapOutcome::Absent => panic!("present map must yield an identity"),
    }
    // An absent map is recorded as Absent — the map-Option-None path never
    // panics ($/getCompiledCode-style map-absent handling).
    assert_eq!(classify_source_map("p", None), MapOutcome::Absent);
}

// ── the recorded map tracks the rewritten generated code ─────────────────

#[test]
fn source_map_shift_resolves_post_rewrite_offset_to_correct_source() {
    use oxc_sourcemap::{SourceMap, Token};
    use std::borrow::Cow;

    // Generated TSX: a `.vue` reexport, then a probe-target token AFTER it on
    // the SAME line (so the rewrite's byte-length change shifts the target).
    let original_code = "export { default as X } from './X.vue';export const greeting = 1";
    let x_col = original_code.find("as X").expect("X") as u32 + "as ".len() as u32;
    let spec_col = original_code.find("'./X.vue'").expect("specifier") as u32;
    let greeting_col = original_code.find("greeting").expect("greeting") as u32;

    // A V3 map: the import name and specifier (both BEFORE the insertion) and
    // the post-rewrite target `greeting` (AFTER the insertion).
    let tokens = vec![
        Token::new(0, x_col, 5, 0, Some(0), None),
        Token::new(0, spec_col, 6, 0, Some(0), None),
        Token::new(0, greeting_col, 10, 5, Some(0), None),
    ]
    .into_boxed_slice();
    let map = SourceMap::new(
        None,
        vec![],
        None,
        vec![Cow::Borrowed("X.vue")],
        vec![None],
        tokens,
        None,
    );
    let map_json = map.to_json_string();

    // Apply the tracked rewrite and shift the map to match it.
    let rewrite = rewrite_vue_imports_tracked(original_code);
    assert!(rewrite.output.contains("./X.vue.ts"), "rewrite must run");
    assert_eq!(rewrite.insertions.len(), 1, "exactly one .vue specifier");
    let rewritten_greeting_col = rewrite.output.find("greeting").expect("greeting") as u32;
    assert_eq!(
        rewritten_greeting_col,
        greeting_col + VUE_TWIN_SUFFIX.len() as u32,
        "the target shifted right by the inserted suffix length"
    );

    let shifted_json =
        shift_source_map_for_insertions(&map_json, original_code, &rewrite.insertions);

    // The SHIFTED map resolves the post-rewrite target offset EXACTLY to its
    // original source position.
    let shifted = SourceMap::from_json_string(&shifted_json).expect("parse shifted");
    let lt = shifted.generate_lookup_table();
    let tok = shifted
        .lookup_token(&lt, 0, rewritten_greeting_col)
        .expect("token at post-rewrite target");
    assert_eq!(
        tok.get_dst_col(),
        rewritten_greeting_col,
        "shifted map has a token EXACTLY at the post-rewrite target column"
    );
    assert_eq!(
        (tok.get_src_line(), tok.get_src_col()),
        (10, 5),
        "post-rewrite target maps to the correct original source position"
    );
    // A token BEFORE the insertion is unchanged.
    let x_tok = shifted.lookup_token(&lt, 0, x_col).expect("x token");
    assert_eq!(
        x_tok.get_dst_col(),
        x_col,
        "a pre-insertion token must not shift"
    );
    assert_eq!((x_tok.get_src_line(), x_tok.get_src_col()), (5, 0));

    // Discrimination: the UN-shifted (host) map cannot exactly locate the
    // post-rewrite target — its `greeting` token is still at the OLD column,
    // so a probe at the post-rewrite offset lands off-by-suffix-length.
    let orig = SourceMap::from_json_string(&map_json).expect("parse orig");
    let olt = orig.generate_lookup_table();
    let off_tok = orig
        .lookup_token(&olt, 0, rewritten_greeting_col)
        .expect("floor token");
    assert_ne!(
        off_tok.get_dst_col(),
        rewritten_greeting_col,
        "the unshifted map has NO token at the post-rewrite target (the bug)"
    );
}

#[test]
fn source_map_shift_is_cumulative_across_multiple_insertions_on_one_line() {
    use oxc_sourcemap::{SourceMap, Token};
    use std::borrow::Cow;

    // TWO `.vue` specifiers on ONE generated line, with a token between them
    // and a token after both — the cumulative (2×) shift must apply past the
    // second insertion, and a token sitting EXACTLY at an insertion column must
    // shift too (the `col >= c` boundary).
    let original_code = "export {a} from './A.vue';export {b} from './B.vue';const z=1";
    let rewrite = rewrite_vue_imports_tracked(original_code);
    assert_eq!(
        rewrite.insertions.len(),
        2,
        "two specifiers → two insertions"
    );
    let ins1 = rewrite.insertions[0] as u32; // closing-quote col of './A.vue'
    let ins2 = rewrite.insertions[1] as u32; // closing-quote col of './B.vue'
    assert!(ins1 < ins2, "ins1 {ins1} < ins2 {ins2}");
    let suffix = VUE_TWIN_SUFFIX.len() as u32; // 3

    // Tokens in the ORIGINAL generated coordinate system, each tagged by a
    // unique source row so it can be identified after the shift.
    let before = 0u32; // before both insertions
    let at_ins1 = ins1; // EXACTLY at insertion 1 (boundary: col == c)
    let between = ins1 + 5; // strictly between the two insertions
    let at_ins2 = ins2; // EXACTLY at insertion 2 (both insertions at/before)
    let after = ins2 + 4; // after both insertions
    assert!(between < ins2, "the between-token must fall before ins2");
    let tokens = vec![
        Token::new(0, before, 1, 0, Some(0), None),
        Token::new(0, at_ins1, 2, 0, Some(0), None),
        Token::new(0, between, 3, 0, Some(0), None),
        Token::new(0, at_ins2, 4, 0, Some(0), None),
        Token::new(0, after, 5, 0, Some(0), None),
    ]
    .into_boxed_slice();
    let map = SourceMap::new(
        None,
        vec![],
        None,
        vec![Cow::Borrowed("x")],
        vec![None],
        tokens,
        None,
    );
    let shifted_json =
        shift_source_map_for_insertions(&map.to_json_string(), original_code, &rewrite.insertions);
    let shifted = SourceMap::from_json_string(&shifted_json).expect("parse shifted");

    // Expected post-shift generated column, keyed by source row.
    let expect = |src_row: u32| -> u32 {
        match src_row {
            1 => before,               // before everything → unchanged
            2 => at_ins1 + suffix,     // boundary at ins1 (>=) → one shift
            3 => between + suffix,     // one insertion before it
            4 => at_ins2 + 2 * suffix, // boundary at ins2 → both insertions count
            5 => after + 2 * suffix,   // cumulative 2× past both
            _ => unreachable!(),
        }
    };
    let mut seen = 0;
    for tok in shifted.get_tokens() {
        let row = tok.get_src_line();
        assert_eq!(
            tok.get_dst_col(),
            expect(row),
            "src row {row}: shifted dst col mismatch"
        );
        seen += 1;
    }
    assert_eq!(seen, 5, "all five tokens survived the shift");

    // Discrimination: a single (non-cumulative) shift would move the trailing
    // token by only +3, never the +6 the cumulative math produces.
    assert_ne!(
        expect(5),
        after + suffix,
        "the cumulative shift must exceed a single-insertion shift"
    );
}

#[test]
fn shift_source_map_is_noop_without_insertions_and_safe_on_garbage() {
    use oxc_sourcemap::{SourceMap, Token};
    use std::borrow::Cow;
    let tokens = vec![Token::new(0, 5, 1, 0, Some(0), None)].into_boxed_slice();
    let map = SourceMap::new(
        None,
        vec![],
        None,
        vec![Cow::Borrowed("a.ts")],
        vec![None],
        tokens,
        None,
    );
    let json = map.to_json_string();
    // No insertions → identical map back.
    assert_eq!(
        shift_source_map_for_insertions(&json, "const x = 1", &[]),
        json
    );
    // A malformed map is returned unchanged (best-effort, never dropped).
    assert_eq!(
        shift_source_map_for_insertions("not a source map", "x", &[0]),
        "not a source map"
    );
}

#[test]
fn byte_offset_to_line_utf16col_counts_utf16_units() {
    // `é` is 2 UTF-8 bytes but 1 UTF-16 code unit.
    let text = "café\nxy";
    // 'f' (byte 2) on line 0 → col 2.
    assert_eq!(byte_offset_to_line_utf16col(text, 2), (0, 2));
    // End of `café` (byte 5, the newline) → line 0, col 4 (c,a,f,é).
    assert_eq!(byte_offset_to_line_utf16col(text, 5), (0, 4));
    // 'y' on line 1 (byte 7) → line 1, col 1.
    assert_eq!(byte_offset_to_line_utf16col(text, 7), (1, 1));
}

#[test]
fn artifact_path_appends_to_full_name() {
    let p = artifact_path(Path::new("/ws/Foo.vue"), ".tsx");
    assert!(p.ends_with("Foo.vue.tsx"), "{p:?}");
    let t = artifact_path(Path::new("/ws/Foo.vue"), ".ts");
    assert!(t.ends_with("Foo.vue.ts"), "{t:?}");
}

// ── @verter/types + vendored shims (no runtime install) ───────────────

#[test]
fn injects_verter_types_and_copies_vendored_vue_without_install() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    // A vendored vue shim that exports `ref` — proves resolution is possible
    // off the committed shim, with no `npm install`.
    let vendor = root.join("vendor").join("node_modules");
    write(
        &vendor.join("vue").join("index.d.ts"),
        "export declare function ref<T>(v: T): { value: T };\n",
    );
    write(
        &vendor.join("vue").join("package.json"),
        r#"{ "name": "vue", "version": "3.5.0", "types": "index.d.ts" }"#,
    );

    let entry = root.join("Entry.vue");
    write(
            &entry,
            "<script setup lang=\"ts\">\nimport { ref } from 'vue'\nconst n = ref(0)\n</script>\n<template><div>{{ n }}</div></template>\n",
        );

    let report = materialize(&MaterializeRequest {
        workspace_root: root.to_path_buf(),
        entries: vec![entry.clone()],
        vendor_node_modules: Some(vendor.clone()),
        expected_vue_version: None,
        strict_vue_version: false,
    })
    .unwrap();

    // @verter/types injected verbatim from the Rust constant.
    let dts = root
        .join("node_modules")
        .join("@verter")
        .join("types")
        .join("index.d.ts");
    assert!(dts.exists(), "@verter/types/index.d.ts must be present");
    assert_eq!(
        fs::read_to_string(&dts).unwrap(),
        VERTER_TYPES_STANDALONE_DTS,
        "must reuse the Rust constant, not a hand-written d.ts"
    );

    // Vendored vue copied into node_modules (no install).
    let vue_dts = root.join("node_modules").join("vue").join("index.d.ts");
    assert!(vue_dts.exists(), "vendored vue shim must be copied");
    assert!(fs::read_to_string(&vue_dts)
        .unwrap()
        .contains("function ref"));

    // Entry IDE artifact emitted and references @verter/types.
    assert_eq!(report.ide_artifacts.len(), 1);
    let tsx = &report.ide_artifacts[0];
    assert!(tsx.generated_path.ends_with("Entry.vue.tsx"));
    assert!(
        tsx.content.contains("@verter/types"),
        "generated TSX must import from @verter/types"
    );
    // Public-API twin emitted.
    assert_eq!(report.public_api_twins.len(), 1);
    assert!(report.public_api_twins[0]
        .generated_path
        .ends_with("Entry.vue.ts"));
    // Negative: nothing failed to compile.
    assert!(
        report.compile_errors.is_empty(),
        "{:?}",
        report.compile_errors
    );
}

#[test]
fn vendored_verter_types_loses_to_rust_constant_shim() {
    // A vendor overlay that ships a STALE `@verter/types` declaration. The
    // Rust-constant shim must win: `@verter/types` is generated from the
    // exported constant, never a vendored/TS declaration — otherwise the
    // baseline would type-check the generated TSX against the wrong helper
    // declarations.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    const STALE_DTS: &str = "export type StaleVerterHelpers = never; // must not win\n";
    const STALE_PKG: &str = r#"{ "name": "@verter/types", "version": "9.9.9-stale" }"#;
    // Guard the discriminator itself: the sentinels must differ from the
    // authoritative Rust constants, or the assertions would pass vacuously.
    assert_ne!(STALE_DTS, VERTER_TYPES_STANDALONE_DTS);
    assert_ne!(STALE_PKG, VERTER_TYPES_PACKAGE_JSON);

    let vendor = root.join("vendor").join("node_modules");
    write(
        &vendor.join("@verter").join("types").join("index.d.ts"),
        STALE_DTS,
    );
    write(
        &vendor.join("@verter").join("types").join("package.json"),
        STALE_PKG,
    );

    write(&root.join("A.vue"), "<template><div/></template>\n");

    materialize(&MaterializeRequest {
        workspace_root: root.to_path_buf(),
        entries: vec![],
        vendor_node_modules: Some(vendor),
        expected_vue_version: None,
        strict_vue_version: false,
    })
    .unwrap();

    // The Rust-constant shim is authoritative — the vendored overlay's
    // stale `@verter/types` declaration is overwritten, not preserved.
    let dts = root
        .join("node_modules")
        .join("@verter")
        .join("types")
        .join("index.d.ts");
    assert_eq!(
        fs::read_to_string(&dts).unwrap(),
        VERTER_TYPES_STANDALONE_DTS,
        "vendored @verter/types/index.d.ts must lose to the Rust constant"
    );
    let pkg = root
        .join("node_modules")
        .join("@verter")
        .join("types")
        .join("package.json");
    assert_eq!(
        fs::read_to_string(&pkg).unwrap(),
        VERTER_TYPES_PACKAGE_JSON,
        "vendored @verter/types/package.json must lose to the Rust constant"
    );
}

// ── vendored Vue declaration version-sync ─────────────────────────────

#[test]
fn vendored_vue_version_sync_matches_passes_and_strict_mismatch_hard_fails() {
    // Build a vendor node_modules with `vue` + `@vue/compiler-core` pinned at
    // explicit versions, then a trivial fixture to materialize.
    fn vendor_at(root: &Path, vue_ver: &str, compiler_ver: &str) -> PathBuf {
        let vendor = root.join("vendor").join("node_modules");
        write(
            &vendor.join("vue").join("package.json"),
            &format!(r#"{{ "name": "vue", "version": "{vue_ver}" }}"#),
        );
        write(
            &vendor.join("vue").join("index.d.ts"),
            "export declare const x: number;\n",
        );
        write(
            &vendor
                .join("@vue")
                .join("compiler-core")
                .join("package.json"),
            &format!(r#"{{ "name": "@vue/compiler-core", "version": "{compiler_ver}" }}"#),
        );
        vendor
    }

    // Matching versions → materialize ok, no warnings.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let vendor = vendor_at(root, "3.5.13", "3.5.13");
    write(&root.join("A.vue"), "<template><div/></template>\n");
    let ok = materialize(&MaterializeRequest {
        workspace_root: root.to_path_buf(),
        entries: vec![],
        vendor_node_modules: Some(vendor),
        expected_vue_version: Some("3.5.13".to_string()),
        strict_vue_version: true,
    })
    .expect("matching vendored Vue versions must materialize");
    assert!(
        ok.vue_version_warnings.is_empty(),
        "no warnings on an exact version match: {:?}",
        ok.vue_version_warnings
    );

    // A drifting `@vue/*` under strict → hard error naming the package + versions.
    let tmp2 = tempfile::tempdir().unwrap();
    let root2 = tmp2.path();
    let vendor2 = vendor_at(root2, "3.5.13", "3.4.0"); // compiler-core drifts
    write(&root2.join("A.vue"), "<template><div/></template>\n");
    let err = materialize(&MaterializeRequest {
        workspace_root: root2.to_path_buf(),
        entries: vec![],
        vendor_node_modules: Some(vendor2),
        expected_vue_version: Some("3.5.13".to_string()),
        strict_vue_version: true,
    })
    .unwrap_err();
    match err {
        MaterializeError::VueVersionMismatch {
            package,
            expected,
            found,
        } => {
            assert_eq!(package, "@vue/compiler-core");
            assert_eq!(expected, "3.5.13");
            assert_eq!(found, "3.4.0");
        }
        other => panic!("expected a hard VueVersionMismatch under strict, got {other:?}"),
    }

    // The SAME drift in non-strict → recorded structured warning, never an error.
    let tmp3 = tempfile::tempdir().unwrap();
    let root3 = tmp3.path();
    let vendor3 = vendor_at(root3, "3.5.13", "3.4.0");
    write(&root3.join("A.vue"), "<template><div/></template>\n");
    let report = materialize(&MaterializeRequest {
        workspace_root: root3.to_path_buf(),
        entries: vec![],
        vendor_node_modules: Some(vendor3),
        expected_vue_version: Some("3.5.13".to_string()),
        strict_vue_version: false,
    })
    .expect("non-strict records a warning, never errors");
    assert!(
        report
            .vue_version_warnings
            .iter()
            .any(|w| w.package == "@vue/compiler-core"
                && w.expected == "3.5.13"
                && w.found == "3.4.0"),
        "non-strict mismatch must be recorded: {:?}",
        report.vue_version_warnings
    );
    // Negative: the matching `vue` core is NOT warned about.
    assert!(
        !report
            .vue_version_warnings
            .iter()
            .any(|w| w.package == "vue"),
        "a matching vue core must not be warned: {:?}",
        report.vue_version_warnings
    );
}

#[test]
fn strict_vue_version_sync_hard_fails_when_required_vue_core_is_absent() {
    // The strict contract REQUIRES the vendored `vue/package.json` version be
    // read and compared. A vendor that copies a matching `@vue/*` line but NO
    // `vue` core must NOT silently pass (an empty/short iteration returning Ok):
    // the missing required core declaration is itself a strict mismatch.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let vendor = root.join("vendor").join("node_modules");
    // A matching `@vue/compiler-core`, but deliberately NO `vue/package.json`.
    write(
        &vendor
            .join("@vue")
            .join("compiler-core")
            .join("package.json"),
        r#"{ "name": "@vue/compiler-core", "version": "3.5.13" }"#,
    );
    write(&root.join("A.vue"), "<template><div/></template>\n");

    let err = materialize(&MaterializeRequest {
        workspace_root: root.to_path_buf(),
        entries: vec![],
        vendor_node_modules: Some(vendor),
        expected_vue_version: Some("3.5.13".to_string()),
        strict_vue_version: true,
    })
    .expect_err("a missing required `vue/package.json` must hard-fail under strict");
    match err {
        MaterializeError::VueVersionMismatch {
            package,
            expected,
            found,
        } => {
            assert_eq!(
                package, "vue",
                "the missing required package is the vue core"
            );
            assert_eq!(expected, "3.5.13");
            assert_eq!(
                found, "<absent>",
                "a missing/unreadable package.json surfaces as <absent>"
            );
        }
        other => {
            panic!("expected a hard VueVersionMismatch for the absent vue core, got {other:?}")
        }
    }
}

#[test]
fn nonstrict_vue_version_sync_records_warning_when_required_vue_core_is_absent() {
    // The SAME absent-core case in non-strict mode → a recorded structured
    // warning naming `vue` with `found = "<absent>"`, never an error.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let vendor = root.join("vendor").join("node_modules");
    write(
        &vendor
            .join("@vue")
            .join("compiler-core")
            .join("package.json"),
        r#"{ "name": "@vue/compiler-core", "version": "3.5.13" }"#,
    );
    write(&root.join("A.vue"), "<template><div/></template>\n");

    let report = materialize(&MaterializeRequest {
        workspace_root: root.to_path_buf(),
        entries: vec![],
        vendor_node_modules: Some(vendor),
        expected_vue_version: Some("3.5.13".to_string()),
        strict_vue_version: false,
    })
    .expect("non-strict records a warning, never errors");
    assert!(
        report
            .vue_version_warnings
            .iter()
            .any(|w| w.package == "vue" && w.expected == "3.5.13" && w.found == "<absent>"),
        "a missing required vue core must be recorded as a <absent> warning: {:?}",
        report.vue_version_warnings
    );
    // Negative: the matching `@vue/compiler-core` is NOT warned about.
    assert!(
        !report
            .vue_version_warnings
            .iter()
            .any(|w| w.package == "@vue/compiler-core"),
        "a matching @vue/* package must not be warned: {:?}",
        report.vue_version_warnings
    );
}

// ── transitive closure: direct child + barrel-reexported child ─────────

#[test]
fn transitive_closure_produces_twins_for_child_and_barrel_reexport() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    // Entry imports a direct child AND a barrel that re-exports a panel.
    write(
            &root.join("Entry.vue"),
            "<script setup lang=\"ts\">\nimport Child from './Child.vue'\nimport { Panel } from './components'\n</script>\n<template><Child /><Panel /></template>\n",
        );
    write(
            &root.join("Child.vue"),
            "<script setup lang=\"ts\">\nconst label = 'child'\n</script>\n<template><span>{{ label }}</span></template>\n",
        );
    write(
            &root.join("components").join("Panel.vue"),
            "<script setup lang=\"ts\">\nconst title = 'panel'\n</script>\n<template><h1>{{ title }}</h1></template>\n",
        );
    // Barrel re-exporting the panel.
    write(
        &root.join("components").join("index.ts"),
        "export { default as Panel } from './Panel.vue'\n",
    );

    let report = materialize(&MaterializeRequest {
        workspace_root: root.to_path_buf(),
        entries: vec![root.join("Entry.vue")],
        vendor_node_modules: None,
        expected_vue_version: None,
        strict_vue_version: false,
    })
    .unwrap();

    let twin_names: Vec<String> = report
        .public_api_twins
        .iter()
        .map(|a| {
            a.generated_path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_string()
        })
        .collect();

    // The imported child AND the barrel-reexported child both got .vue.ts twins.
    assert!(
        twin_names.contains(&"Entry.vue.ts".to_string()),
        "{twin_names:?}"
    );
    assert!(
        twin_names.contains(&"Child.vue.ts".to_string()),
        "imported child twin missing: {twin_names:?}"
    );
    assert!(
        twin_names.contains(&"Panel.vue.ts".to_string()),
        "barrel-reexported child twin missing: {twin_names:?}"
    );

    // Entry's TSX rewrites the .vue import to .vue.ts on disk.
    let entry_tsx = report
        .ide_artifacts
        .iter()
        .find(|a| a.generated_path.ends_with("Entry.vue.tsx"))
        .expect("entry tsx");
    assert!(
        entry_tsx.content.contains("Child.vue.ts"),
        "entry TSX must import the rewritten child twin"
    );
    // Negative: no raw `.vue'` specifier survives the rewrite.
    assert!(
        !entry_tsx.content.contains("./Child.vue'"),
        "raw .vue specifier must be rewritten"
    );

    // Twins are real declarations, not empty.
    for twin in &report.public_api_twins {
        assert!(
            !twin.content.trim().is_empty(),
            "empty twin: {:?}",
            twin.generated_path
        );
    }

    // The on-disk barrel's `./Panel.vue` reexport is rewritten to the twin,
    // so the provider resolves the reexport THROUGH `Panel.vue.ts` rather
    // than a raw `.vue` path it cannot resolve.
    let barrel = fs::read_to_string(root.join("components").join("index.ts")).unwrap();
    assert!(
        barrel.contains("./Panel.vue.ts"),
        "barrel reexport must be rewritten to the twin: {barrel:?}"
    );
    // Negative: no raw `./Panel.vue'` specifier survives in the barrel.
    assert!(
        !barrel.contains("./Panel.vue'"),
        "raw .vue reexport must not survive the rewrite: {barrel:?}"
    );
    // The rewrite is recorded for the runner.
    assert!(
        report
            .support_rewrites
            .iter()
            .any(|p| p.ends_with("index.ts")),
        "barrel rewrite must be recorded: {:?}",
        report.support_rewrites
    );
}

#[test]
fn support_file_rewrite_skips_string_literals_and_comments_end_to_end() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    // A child .vue so its twin exists for the rewritten specifier to resolve.
    write(
            &root.join("Child.vue"),
            "<script setup lang=\"ts\">\nconst label = 'child'\n</script>\n<template><span/></template>\n",
        );
    // A support .ts barrel carrying a real reexport specifier AND a
    // non-specifier `.vue` string literal + a comment that both mention `.vue`
    // immediately before a closing quote — the shape a bare before-quote scan
    // would wrongly rewrite, but a specifier-aware rewrite must leave intact.
    write(
        &root.join("barrel.ts"),
        concat!(
            "export { default as Child } from './Child.vue'\n",
            "export const note = './Child.vue'\n",
            "// fallback path: './Child.vue'\n",
        ),
    );

    let report = materialize(&MaterializeRequest {
        workspace_root: root.to_path_buf(),
        entries: vec![],
        vendor_node_modules: None,
        expected_vue_version: None,
        strict_vue_version: false,
    })
    .unwrap();

    let barrel = fs::read_to_string(root.join("barrel.ts")).unwrap();
    // The reexport specifier IS rewritten to the twin.
    assert!(
        barrel.contains("export { default as Child } from './Child.vue.ts'"),
        "reexport specifier must be rewritten: {barrel}"
    );
    // The plain string assignment (not a specifier) is UNCHANGED.
    assert!(
        barrel.contains("export const note = './Child.vue'\n"),
        "non-specifier string literal must not be rewritten: {barrel}"
    );
    // The comment is UNCHANGED.
    assert!(
        barrel.contains("// fallback path: './Child.vue'"),
        "comment must not be rewritten: {barrel}"
    );
    // Exactly ONE `.vue.ts` exists — only the specifier gained it; the literal
    // and the comment did not.
    assert_eq!(
        barrel.matches(".vue.ts").count(),
        1,
        "only the import specifier may gain `.vue.ts`: {barrel}"
    );
    // The rewrite was recorded for the runner.
    assert!(
        report
            .support_rewrites
            .iter()
            .any(|p| p.ends_with("barrel.ts")),
        "barrel rewrite must be recorded: {:?}",
        report.support_rewrites
    );
}

#[test]
fn ide_artifact_records_a_shifted_source_map_consistent_with_rewritten_code() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    // Entry imports a child `.vue` → the generated TSX carries a `.vue`
    // specifier that materialization rewrites to `.vue.ts`.
    write(
            &root.join("Entry.vue"),
            "<script setup lang=\"ts\">\nimport Child from './Child.vue'\nconst greeting: string = 'hi'\n</script>\n<template><Child />{{ greeting }}</template>\n",
        );
    write(
            &root.join("Child.vue"),
            "<script setup lang=\"ts\">\nconst label = 'child'\n</script>\n<template><span>{{ label }}</span></template>\n",
        );
    let report = materialize(&MaterializeRequest {
        workspace_root: root.to_path_buf(),
        entries: vec![root.join("Entry.vue")],
        vendor_node_modules: None,
        expected_vue_version: None,
        strict_vue_version: false,
    })
    .unwrap();

    let entry = report
        .ide_artifacts
        .iter()
        .find(|a| a.generated_path.ends_with("Entry.vue.tsx"))
        .expect("entry tsx");
    // The rewrite happened in the recorded content.
    assert!(entry.content.contains("Child.vue.ts"));
    assert!(!entry.content.contains("./Child.vue'"));
    // When the host produced a map, the recorded map is present AND well-formed
    // V3 JSON (the shift round-tripped it through oxc_sourcemap), so a
    // position-resolving consumer reads a map consistent with the rewrite.
    if entry.source_map_present {
        let map_json = entry
            .source_map
            .as_deref()
            .expect("source_map_present implies a recorded map");
        let parsed = oxc_sourcemap::SourceMap::from_json_string(map_json)
            .expect("recorded map must be valid V3 JSON");
        assert!(
            parsed.get_tokens().count() > 0,
            "the shifted map must retain its generated tokens"
        );
    }
}

#[test]
fn rewrite_vue_imports_leaves_wildcard_module_glob_intact() {
    // A glob module declaration must NOT be corrupted into `*.vue.ts`.
    let shim = "declare module '*.vue' { const c: unknown; export default c }";
    assert_eq!(rewrite_vue_imports(shim), shim);
    // A concrete reexport in the same family IS rewritten (both quote forms).
    assert_eq!(
        rewrite_vue_imports("export { default as Panel } from './Panel.vue'"),
        "export { default as Panel } from './Panel.vue.ts'"
    );
    assert_eq!(
        rewrite_vue_imports("from \"./Panel.vue\""),
        "from \"./Panel.vue.ts\""
    );
    // A `.vue`-prefixed but non-specifier-ending name is left untouched.
    assert_eq!(
        rewrite_vue_imports("import x from './theme.vuetify'"),
        "import x from './theme.vuetify'"
    );
}

#[test]
fn rewrite_only_touches_import_specifiers_not_string_literals_or_comments() {
    // A support `.ts` carrying a real import specifier AND a non-specifier
    // `.vue` string literal / comments. Only the specifier may be rewritten —
    // an ordinary string literal or a comment that mentions `.vue` must stay
    // byte-for-byte intact (rewriting it would change TS semantics).
    let src = concat!(
        "import Child from \"./Child.vue\"\n",
        "const label = \"see ./Child.vue\"\n",
        "// import Other from \"./Other.vue\"\n",
        "/* block ./Block.vue mention */\n",
        "export { default as P } from './P.vue'\n",
        "const dyn = () => import('./Lazy.vue')\n",
    );
    let out = rewrite_vue_imports(src);

    // Real static import specifier → rewritten.
    assert!(
        out.contains("import Child from \"./Child.vue.ts\""),
        "static import specifier must be rewritten: {out}"
    );
    // Reexport specifier → rewritten.
    assert!(
        out.contains("export { default as P } from './P.vue.ts'"),
        "reexport specifier must be rewritten: {out}"
    );
    // Dynamic import() specifier → rewritten.
    assert!(
        out.contains("import('./Lazy.vue.ts')"),
        "dynamic import specifier must be rewritten: {out}"
    );
    // Ordinary string literal → UNCHANGED.
    assert!(
        out.contains("const label = \"see ./Child.vue\""),
        "string literal must not be rewritten: {out}"
    );
    assert!(
        !out.contains("see ./Child.vue.ts"),
        "the string-literal `.vue` must not gain a `.ts`: {out}"
    );
    // Line comment → UNCHANGED.
    assert!(
        out.contains("// import Other from \"./Other.vue\""),
        "line comment must not be rewritten: {out}"
    );
    assert!(
        !out.contains("./Other.vue.ts"),
        "the commented `.vue` must not gain a `.ts`: {out}"
    );
    // Block comment → UNCHANGED.
    assert!(
        out.contains("/* block ./Block.vue mention */"),
        "block comment must not be rewritten: {out}"
    );
    assert!(
        !out.contains("./Block.vue.ts"),
        "the block-commented `.vue` must not gain a `.ts`: {out}"
    );
}

#[test]
fn rewrite_leaves_regex_and_division_untouched_but_rewrites_real_imports() {
    // A regex literal whose body contains a balanced-quote `.vue` path right
    // after the word `from`. A regex-blind lexer mis-reads the `'./Child.vue'`
    // quote run as an import specifier and wrongly appends `.ts`; a regex-aware
    // lexer consumes the whole literal so its interior never becomes a string
    // token. The `/` here is in regex position (it follows `=`).
    let regex_line = "const r = /from './Child.vue'/";
    assert_eq!(
        rewrite_vue_imports(regex_line),
        regex_line,
        "a `.vue` quote run inside a regex literal must never be rewritten"
    );

    // A regex (with a `from '…vue'` shape inside) on one line, then a REAL
    // import on the next. The regex stays verbatim and the import is rewritten;
    // the regex scan is newline-bounded, so it cannot swallow the import line.
    let mixed = concat!("const re = /from 'X.vue'/\n", "import B from './B.vue'\n");
    let out = rewrite_vue_imports(mixed);
    assert!(
        out.contains("/from 'X.vue'/\n"),
        "the regex literal must be left byte-for-byte: {out}"
    );
    assert!(
        !out.contains("X.vue.ts"),
        "the regex-interior specifier-shaped quote must not gain `.ts`: {out}"
    );
    assert!(
        out.contains("import B from './B.vue.ts'"),
        "a real import after a regex must still be rewritten: {out}"
    );

    // Division (`a / b`) is NOT a regex: the operands stay intact (the `/`
    // follows an identifier) and a following real import is still rewritten.
    let div = concat!("const n = a / b\n", "import C from \"./C.vue\"\n");
    let outd = rewrite_vue_imports(div);
    assert!(outd.contains("a / b"), "division must be untouched: {outd}");
    assert!(
        outd.contains("import C from \"./C.vue.ts\""),
        "a real import after a division must be rewritten: {outd}"
    );
}

#[test]
fn synthesizes_tsconfig_when_absent_and_keeps_existing() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write(&root.join("A.vue"), "<template><div/></template>\n");

    let report = materialize(&MaterializeRequest {
        workspace_root: root.to_path_buf(),
        entries: vec![],
        vendor_node_modules: None,
        expected_vue_version: None,
        strict_vue_version: false,
    })
    .unwrap();
    assert!(report.synthesized_tsconfig);
    let cfg = fs::read_to_string(root.join("tsconfig.json")).unwrap();
    assert!(cfg.contains("\"jsxImportSource\": \"vue\""));
    assert!(cfg.contains("\"allowArbitraryExtensions\": true"));

    // Second run keeps the existing tsconfig (does not re-synthesize).
    let report2 = materialize(&MaterializeRequest {
        workspace_root: root.to_path_buf(),
        entries: vec![],
        vendor_node_modules: None,
        expected_vue_version: None,
        strict_vue_version: false,
    })
    .unwrap();
    assert!(!report2.synthesized_tsconfig);
}

#[test]
fn bad_root_is_rejected() {
    let err = materialize(&MaterializeRequest {
        workspace_root: PathBuf::from("/no/such/dir/verter-dx-xyz"),
        entries: vec![],
        vendor_node_modules: None,
        expected_vue_version: None,
        strict_vue_version: false,
    })
    .unwrap_err();
    assert!(matches!(err, MaterializeError::BadRoot(_)));
}
