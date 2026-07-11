//! Guard: `carrier_ownership_extension_rules`.
//!
//! Adapter-parameterized assertion of the §2.6 TS-equivalent extension rule for
//! carrier-source ownership, for `.vue` AND `.svelte` (and any registered
//! adapter's carrier extension). A carrier source is owned by a configured
//! project ONLY via: the default include, a no-extension directory / bare-star
//! glob, or a glob/`files` entry that explicitly covers the carrier extension —
//! an extension-specific `*.ts` glob does NOT own it. NO brace expansion:
//! multi-extension coverage is SEPARATE entries, never `*.{vue,svelte}`.
//!
//! DISCRIMINATING: against a literal-glob model (no extension expansion) case 3
//! would wrongly OWN the carrier under `*.ts`, and the negative control would
//! own an unknown extension under bare-star; against the explicit-extension
//! model the assertions below hold.

use verter_session::external_ts::CarrierOwnershipResolution;

use crate::shared::{carrier_exts, resolve_with};

const TS: &str = "d:/ws/tsconfig.json";

fn is_owned(res: &CarrierOwnershipResolution) -> bool {
    matches!(res, CarrierOwnershipResolution::Bound(_))
}

fn is_no_project(res: &CarrierOwnershipResolution) -> bool {
    matches!(res, CarrierOwnershipResolution::NoProject)
}

/// Resolve a carrier source `src/Foo.<ext>` under the given tsconfig JSON body.
fn resolve_carrier(tsconfig_body: &str, ext: &str) -> CarrierOwnershipResolution {
    let source = format!("d:/ws/src/Foo.{ext}");
    resolve_with(
        &[(TS, tsconfig_body), (source.as_str(), "// carrier source")],
        &[TS],
        &source,
    )
}

#[test]
fn carrier_ownership_extension_rules() {
    let exts = carrier_exts();
    assert!(
        exts.iter().any(|e| e == "vue") && exts.iter().any(|e| e == "svelte"),
        "this guard requires the built-in `.vue` AND `.svelte` carrier adapters; \
         registered carriers were {exts:?}"
    );

    for ext in &exts {
        // Case 1 — `include: ["src"]` (directory glob) OWNS the carrier.
        assert!(
            is_owned(&resolve_carrier(r#"{ "include": ["src"] }"#, ext)),
            "case 1: directory glob `[\"src\"]` must own `Foo.{ext}`"
        );

        // Case 2 — `include: ["src/**/*"]` (bare star) OWNS the carrier.
        assert!(
            is_owned(&resolve_carrier(r#"{ "include": ["src/**/*"] }"#, ext)),
            "case 2: bare-star glob `[\"src/**/*\"]` must own `Foo.{ext}`"
        );

        // Case 3 — `include: ["src/**/*.ts"]` (extension-specific) does NOT own
        // ⇒ NoProject.
        assert!(
            is_no_project(&resolve_carrier(r#"{ "include": ["src/**/*.ts"] }"#, ext)),
            "case 3: extension-specific `[\"src/**/*.ts\"]` must NOT own `Foo.{ext}` ⇒ NoProject"
        );

        // Case 4 — SEPARATE per-extension entries (NO brace expansion) OWN the
        // carrier. Built dynamically so every registered carrier is covered.
        let mut entries: Vec<String> = vec!["\"src/**/*.ts\"".to_string()];
        for e in &exts {
            entries.push(format!("\"src/**/*.{e}\""));
        }
        let case4 = format!(r#"{{ "include": [{}] }}"#, entries.join(", "));
        assert!(
            is_owned(&resolve_carrier(&case4, ext)),
            "case 4: separate per-extension entries must own `Foo.{ext}` (NO brace expansion)"
        );

        // Case 5 — default-include (no `files`/`include`) OWNS the carrier.
        assert!(
            is_owned(&resolve_carrier(r#"{ "compilerOptions": {} }"#, ext)),
            "case 5: default-include must own `Foo.{ext}`"
        );

        // Case 6 — an `exclude`d carrier source ⇒ NoProject.
        let source = format!("d:/ws/src/excluded/Foo.{ext}");
        let res = resolve_with(
            &[
                (
                    TS,
                    r#"{ "include": ["src/**/*"], "exclude": ["src/excluded"] }"#,
                ),
                (source.as_str(), "// excluded carrier"),
            ],
            &[TS],
            &source,
        );
        assert!(
            is_no_project(&res),
            "case 6: an excluded carrier source `excluded/Foo.{ext}` ⇒ NoProject"
        );

        // Case 7 — an EXCLUDE-ONLY config (no `files`/`include`) keeps the
        // implicit default include MINUS the excludes: it OWNS `src/Foo.{ext}`
        // and REJECTS `dist/Foo.{ext}`. DISCRIMINATING for FIX 1: before the
        // default-include synthesis an exclude-only config owned NOTHING.
        let owned_src = format!("d:/ws/src/Foo.{ext}");
        let owned = resolve_with(
            &[
                (TS, r#"{ "exclude": ["dist"] }"#),
                (owned_src.as_str(), "// carrier under default include"),
            ],
            &[TS],
            &owned_src,
        );
        assert!(
            is_owned(&owned),
            "case 7: exclude-only config (default include) must OWN `src/Foo.{ext}`"
        );

        let excluded_src = format!("d:/ws/dist/Foo.{ext}");
        let excluded = resolve_with(
            &[
                (TS, r#"{ "exclude": ["dist"] }"#),
                (excluded_src.as_str(), "// carrier under exclude"),
            ],
            &[TS],
            &excluded_src,
        );
        assert!(
            is_no_project(&excluded),
            "case 7: exclude-only config must REJECT `dist/Foo.{ext}` ⇒ NoProject"
        );

        // Case 8 — an EXPLICIT `files: []` solution-style config owns NOTHING but
        // its references (distinct from "no files key"): `src/Foo.{ext}` is NOT
        // owned. Discriminates the absent-vs-explicit-empty distinction.
        let solution_src = format!("d:/ws/src/Foo.{ext}");
        let solution = resolve_with(
            &[
                (TS, r#"{ "files": [], "references": [] }"#),
                (solution_src.as_str(), "// carrier under solution-style"),
            ],
            &[TS],
            &solution_src,
        );
        assert!(
            is_no_project(&solution),
            "case 8: explicit `files: []` solution-style must NOT own `src/Foo.{ext}` \
             (owns nothing but references)"
        );
    }
}

/// The brace-expansion ban is structural: a `*.{vue,svelte}` glob is NOT how
/// multi-extension coverage is expressed — separate entries are. This proves the
/// model does not silently rely on a brace glob (which TypeScript does not
/// support in `include`).
#[test]
fn no_brace_expansion_in_include_ownership() {
    let exts = carrier_exts();
    // A brace glob is a literal pattern TS never expands; under our model it is
    // an extension-specific glob (final segment has a `.`), so it is NOT
    // expanded into the supported set and matches its literal text only — i.e.
    // it does NOT own `Foo.vue`. (The point: the model never produces brace
    // coverage; case 4 above is the supported way.)
    let braces = format!(r#"{{ "include": ["src/**/*.{{{}}}"] }}"#, exts.join(","));
    for ext in &exts {
        assert!(
            !is_owned(&resolve_carrier(&braces, ext)),
            "a brace glob `*.{{…}}` must NOT be how `Foo.{ext}` ownership is achieved \
             (no brace expansion; use separate per-extension entries)"
        );
    }
}
