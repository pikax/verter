//! Plain-script dialect authority — runtime half of the
//! `plain_script_dialect_from_file_language` architecture guard (the
//! static-grep half lives in
//! `tests/cases/g_misc0/plain_script_dialect_from_file_language.rs`).
//!
//! Non-carrier (plain Script) files parse under the dialect their
//! classified [`verter_language::FileLanguage`] row declares — the
//! registry is the SOLE plain-script dialect authority. This suite
//! pins:
//!
//!  * `.tsx` / `.jsx` dependencies parse under their JSX dialect and
//!    keep their export surfaces (no degraded empty snapshot);
//!  * the JS module-kind hazard: module `.js` / `.mjs` dependencies
//!    keep parsing `import` / `export` (module-only syntax) — a naive
//!    `Js → SourceType::script()` mapping would regress them;
//!  * `.ts` NEVER sniffs JSX — angle-bracket type assertions stay
//!    TypeScript;
//!  * the `.d.ts` family parses as declaration files through the
//!    registry `Dts` rows (no path re-sniffing in session parse code);
//!  * `HostSourceData::source_type` / `authoritative_source_type_for`
//!    report the classified dialect for every plain-script extension;
//!  * Vue byte-identity spot-checks at FULL `SourceType` fidelity
//!    (language + JSX + module kind): the carrier-side enum shape
//!    change must not move any `<script lang>` row.

use super::*;
use std::sync::Arc;
use verter_language::FileLanguage;

fn make_host() -> VerterHost {
    VerterHost::new_standalone(HostConfig::default())
}

/// Upsert a file under its CLASSIFIED language row — the same row the
/// host's ingress paths (watcher scan, editor open) resolve.
fn upsert_classified(host: &VerterHost, id: &str, src: &str) {
    let file_language = host.language_classifier().classify(id);
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: id.to_string(),
            source: Arc::from(src),
            file_language,
            aliases: Vec::new(),
        })
        .unwrap();
}

fn upsert_vue(host: &VerterHost, id: &str, src: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: id.to_string(),
            source: Arc::from(src),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .unwrap();
}

fn export_names(host: &VerterHost, id: &str) -> Vec<String> {
    let indexed = host
        .ensure_indexed_ready(id)
        .unwrap_or_else(|| panic!("indexed ready must exist for {id}"));
    indexed
        .export_signatures
        .as_ref()
        .map(|sigs| sigs.iter().map(|s| s.name.clone()).collect())
        .unwrap_or_default()
}

fn import_count(host: &VerterHost, id: &str) -> usize {
    let indexed = host
        .ensure_indexed_ready(id)
        .unwrap_or_else(|| panic!("indexed ready must exist for {id}"));
    indexed
        .script_analysis
        .as_ref()
        .map(|sa| sa.imports.len())
        .unwrap_or(0)
}

fn source_type_for(host: &VerterHost, id: &str) -> oxc_span::SourceType {
    host.authoritative_source_type_for(id)
        .unwrap_or_else(|| panic!("authoritative source type must exist for {id}"))
}

// ───────────── classified-dialect parsing (export surfaces) ─────────────

#[test]
fn plain_tsx_dependency_parses_under_tsx_and_keeps_its_export_surface() {
    let host = make_host();
    upsert_classified(
        &host,
        "/src/Button.tsx",
        "export const Button = () => <button>go</button>;\nexport type ButtonProps = { label: string };\n",
    );

    let st = source_type_for(&host, "/src/Button.tsx");
    assert!(
        st.is_typescript() && st.is_jsx(),
        "a .tsx plain script must report the classified TSX dialect, got {st:?}"
    );
    assert!(
        !st.is_typescript_definition(),
        ".tsx is not a declaration file"
    );

    let exports = export_names(&host, "/src/Button.tsx");
    assert!(
        exports.iter().any(|n| n == "Button") && exports.iter().any(|n| n == "ButtonProps"),
        "JSX-bearing .tsx dependency must keep its export surface (no degraded \
         empty snapshot from a plain-TS misparse), got {exports:?}"
    );
}

#[test]
fn plain_jsx_dependency_parses_under_jsx_and_keeps_its_export_surface() {
    let host = make_host();
    upsert_classified(
        &host,
        "/src/Chip.jsx",
        "export const Chip = () => <span>chip</span>;\n",
    );

    let st = source_type_for(&host, "/src/Chip.jsx");
    assert!(
        st.is_javascript() && st.is_jsx(),
        "a .jsx plain script must report the classified JSX dialect, got {st:?}"
    );

    let exports = export_names(&host, "/src/Chip.jsx");
    assert!(
        exports.iter().any(|n| n == "Chip"),
        "JSX-bearing .jsx dependency must keep its export surface, got {exports:?}"
    );
}

// ───────────── JS module-kind hazard pins ────────────────────

#[test]
fn module_js_dependency_keeps_its_import_export_surface() {
    // `import` / `export` are MODULE-ONLY syntax. A naive
    // `Js → SourceType::script()` dialect mapping would turn this
    // file's parse into a syntax error and drop its surfaces.
    let host = make_host();
    upsert_classified(&host, "/src/dep.js", "export const shared = 1;\n");
    upsert_classified(
        &host,
        "/src/consumer.js",
        "import { shared } from './dep.js';\nexport const twice = shared * 2;\n",
    );

    let st = source_type_for(&host, "/src/consumer.js");
    assert!(
        st.is_javascript() && !st.is_jsx() && st.is_unambiguous(),
        "a .js plain script is JavaScript with the unambiguous module kind, got {st:?}"
    );

    let exports = export_names(&host, "/src/consumer.js");
    assert!(
        exports.iter().any(|n| n == "twice"),
        ".js module dependency must keep its export surface, got {exports:?}"
    );
    assert!(
        import_count(&host, "/src/consumer.js") >= 1,
        ".js module dependency must keep its import surface"
    );
}

#[test]
fn mjs_dependency_keeps_its_import_export_surface() {
    let host = make_host();
    upsert_classified(&host, "/src/dep.mjs", "export const shared = 1;\n");
    upsert_classified(
        &host,
        "/src/consumer.mjs",
        "import { shared } from './dep.mjs';\nexport const twice = shared * 2;\n",
    );

    let st = source_type_for(&host, "/src/consumer.mjs");
    assert!(
        st.is_javascript() && st.is_module(),
        "a .mjs plain script is a JavaScript MODULE, got {st:?}"
    );

    let exports = export_names(&host, "/src/consumer.mjs");
    assert!(
        exports.iter().any(|n| n == "twice"),
        ".mjs module dependency must keep its export surface, got {exports:?}"
    );
    assert!(
        import_count(&host, "/src/consumer.mjs") >= 1,
        ".mjs module dependency must keep its import surface"
    );
}

// ───────────── .ts never sniffs JSX ─────────────

#[test]
fn ts_angle_bracket_type_assertion_stays_typescript_not_jsx() {
    // `<number>v` is an angle-bracket type assertion — valid in plain
    // TS, a misparse under TSX grammar. `.ts` NEVER sniffs JSX.
    let host = make_host();
    upsert_classified(
        &host,
        "/src/assert.ts",
        "const v: unknown = 0;\nexport const n = <number>v;\n",
    );

    let st = source_type_for(&host, "/src/assert.ts");
    assert!(
        st.is_typescript() && !st.is_jsx() && !st.is_typescript_definition(),
        "a .ts plain script stays non-JSX TypeScript, got {st:?}"
    );

    let exports = export_names(&host, "/src/assert.ts");
    assert!(
        exports.iter().any(|n| n == "n"),
        "angle-bracket type assertion must parse under TS grammar, got {exports:?}"
    );
}

// ───────────── .d.ts family through the registry Dts rows ─────────────

#[test]
fn dts_family_parses_as_declaration_files_through_the_registry_row() {
    let host = make_host();
    for id in ["/src/a.d.ts", "/src/b.d.mts", "/src/c.d.cts"] {
        upsert_classified(&host, id, "export declare const flag: boolean;\n");
        let st = source_type_for(&host, id);
        assert!(
            st.is_typescript_definition(),
            "{id} must classify as a TypeScript declaration file, got {st:?}"
        );
        let exports = export_names(&host, id);
        assert!(
            exports.iter().any(|n| n == "flag"),
            "{id} must keep its declaration export surface, got {exports:?}"
        );
    }
}

// ───────────── authoritative source-type matrix ─────────────

#[test]
fn authoritative_source_type_reports_the_classified_dialect_per_extension() {
    // (path, source, expectation label, predicate)
    type Pred = fn(oxc_span::SourceType) -> bool;
    let matrix: &[(&str, &str, &str, Pred)] = &[
        (
            "/m/a.ts",
            "export const a = 1;",
            "non-JSX TypeScript",
            |st| st.is_typescript() && !st.is_jsx() && !st.is_typescript_definition(),
        ),
        (
            "/m/a.mts",
            "export const a = 1;",
            "non-JSX TypeScript",
            |st| st.is_typescript() && !st.is_jsx(),
        ),
        (
            "/m/a.cts",
            "export const a = 1;",
            "non-JSX TypeScript",
            |st| st.is_typescript() && !st.is_jsx(),
        ),
        (
            "/m/a.tsx",
            "export const a = 1;",
            "TypeScript + JSX",
            |st| st.is_typescript() && st.is_jsx(),
        ),
        (
            "/m/a.jsx",
            "export const a = 1;",
            "JavaScript + JSX (unambiguous)",
            |st| st.is_javascript() && st.is_jsx() && st.is_unambiguous(),
        ),
        (
            "/m/a.js",
            "export const a = 1;",
            "JavaScript (unambiguous)",
            |st| st.is_javascript() && !st.is_jsx() && st.is_unambiguous(),
        ),
        (
            "/m/a.mjs",
            "export const a = 1;",
            "JavaScript (module)",
            |st| st.is_javascript() && !st.is_jsx() && st.is_module(),
        ),
        (
            "/m/a.cjs",
            "module.exports = { a: 1 };",
            "JavaScript (commonjs)",
            |st| st.is_javascript() && !st.is_jsx() && st.is_commonjs(),
        ),
        (
            "/m/a.d.ts",
            "export declare const a: number;",
            "TypeScript declaration",
            |st| st.is_typescript_definition(),
        ),
        (
            "/m/a.d.mts",
            "export declare const a: number;",
            "TypeScript declaration",
            |st| st.is_typescript_definition(),
        ),
        (
            "/m/a.d.cts",
            "export declare const a: number;",
            "TypeScript declaration",
            |st| st.is_typescript_definition(),
        ),
    ];

    let host = make_host();
    for (id, src, label, pred) in matrix {
        upsert_classified(&host, id, src);
        let st = source_type_for(&host, id);
        assert!(
            pred(st),
            "{id} must report its classified dialect ({label}), got {st:?}"
        );
    }
}

// ───────────── Vue byte-identity spot-checks (full fidelity) ─────────────

#[test]
fn vue_carrier_source_types_are_byte_identical_at_full_fidelity() {
    // The carrier-side `ScriptRegion.source_type` producer follows the
    // neutral enum shape change; the COMPUTED `SourceType` for every
    // `<script lang>` row must not move — language, JSX flag, AND
    // module kind (the render-string matrix in
    // `framework_parse_characterization_tests` does not see module
    // kind, so it is pinned here).
    type Pred = fn(oxc_span::SourceType) -> bool;
    let matrix: &[(&str, &str, &str, Pred)] = &[
        (
            "bi_ts.vue",
            "<script lang=\"ts\">export default {}</script>",
            "SourceType::ts()",
            |st| st == oxc_span::SourceType::ts(),
        ),
        (
            "bi_tsx.vue",
            "<script lang=\"tsx\">export default {}</script>",
            "SourceType::tsx()",
            |st| st == oxc_span::SourceType::tsx(),
        ),
        (
            "bi_jsx.vue",
            "<script lang=\"jsx\">export default {}</script>",
            "SourceType::jsx() (JS module + JSX)",
            |st| st == oxc_span::SourceType::jsx(),
        ),
        (
            "bi_js.vue",
            "<script lang=\"js\">export default {}</script>",
            "SourceType::script() (classic script)",
            |st| st == oxc_span::SourceType::script(),
        ),
        (
            "bi_none.vue",
            "<script>export default {}</script>",
            "SourceType::ts()",
            |st| st == oxc_span::SourceType::ts(),
        ),
    ];

    let host = make_host();
    for (id, src, label, pred) in matrix {
        upsert_vue(&host, id, src);
        let st = source_type_for(&host, id);
        assert!(
            pred(st),
            "Vue carrier source type drifted for {id}: expected {label}, got {st:?}"
        );
    }
}

#[test]
fn cjs_with_esm_syntax_stays_commonjs_and_keeps_a_recovered_surface() {
    // `.cjs` is pinned to the CommonJS grammar. Module-only `export`
    // syntax is erroneous there, but OXC's recovering parser does not
    // panick on it — the declaration still surfaces, so a `.cjs` file
    // that (incorrectly) uses ESM syntax loses nothing vs the old
    // uniform-TS parse. This pins BOTH halves: the classified dialect
    // is CommonJS, and the recovered export surface is preserved.
    let host = make_host();
    upsert_classified(&host, "/src/esm.cjs", "export const nope = 1;\n");

    let st = source_type_for(&host, "/src/esm.cjs");
    assert!(
        st.is_javascript() && st.is_commonjs(),
        "a .cjs plain script is CommonJS JavaScript, got {st:?}"
    );

    let exports = export_names(&host, "/src/esm.cjs");
    assert_eq!(
        exports,
        vec!["nope".to_string()],
        "OXC recovers erroneous ESM syntax in .cjs — the surface must \
         not silently vanish under the CommonJS grammar"
    );
}
