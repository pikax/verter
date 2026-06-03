//! IDE-emit architecture guard: no baked `prefix + identifier` mapped overwrite.
//!
//! The four confirmed go-to-definition desync sites (v-html, v-text, dynamic-key
//! bind `:[key]`, native `v-model`) used to collapse `prefix + user_identifier`
//! into ONE `out.overwrite(prop.start, prop_end, &format!("…{resolved}…"))`. A
//! `Chunk::Overwritten` maps its whole generated run back to the overwrite's
//! source start — so the user identifier mapped to the prop start, breaking
//! hover / go-to-definition. The typed `EmitOp` substrate
//! (`crates/verter_compiler/src/ide/template/emit.rs`) makes that shape
//! impossible; this guard pins it mechanically.
//!
//! Two checks:
//! 1. `no_ide_codegen_bakes_prefix_into_mapped_overwrite` — source scan over
//!    `crates/verter_compiler/src/ide/template/**` forbidding the four desync
//!    sites' specific shape: a mapped `out.overwrite(…)` whose `&format!`
//!    replacement emits a binding-resolved expression as a **bare JSX attribute
//!    value** (`NAME={resolved}` / `NAME={{[…]: resolved}}`). This is the exact
//!    pattern that mapped the user identifier to the prop start.
//! 2. `retired_ide_emit_symbols_absent` — the flat-string producers
//!    `resolve_prefixed_expr` / `resolve_prefixed_dynamic_arg` must be ABSENT from
//!    `crates/verter_compiler/src/**` (they were deleted; a lingering producer
//!    would re-open the desync path).
//!
//! Each check is paired with a discriminating sanity test proving the scanner
//! actually fires on a synthetic violation (so the guard is not vacuous).
//!
//! Scope note (residuals, NOT over-claimed): wrapped / structurally-transformed
//! emit sites — the v-on object-literal spread (`{...{ … }}` via
//! `rewrite_v_on_object_literal_expr`), the v-on `eventCallbacks` / arrow
//! wrappers, the dynamic event-name computed-key spread, v-show's
//! `style={{display: … }}`, and synthesized no-value attribute values — do not
//! emit the expression as a bare navigable JSX value and are out of this phase's
//! mapping scope. They resolve through the shared `build_prefixed_expr` helper
//! (the sanctioned flat-string path) and are intentionally not covered here.

use std::path::{Path, PathBuf};

/// `crates/verter_compiler/src`.
fn compiler_src_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// `crates/verter_compiler/src/ide/template`.
fn ide_template_dir() -> PathBuf {
    compiler_src_dir().join("ide").join("template")
}

/// Retired IDE-only flat-string prefixed-expression producers.
const RETIRED_IDE_EMIT_SYMBOLS: &[&str] = &[
    "fn resolve_prefixed_expr",
    "fn resolve_prefixed_dynamic_arg",
];

/// Recursively collect every `.rs` file under `dir`.
fn rust_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                out.push(path);
            }
        }
    }
    out
}

/// The four desync sites' specific bare-JSX-value emission signatures. Each is a
/// substring that appears ONLY inside the pre-change `&format!` replacement of one
/// of the four sites — a mapped overwrite emitting a binding-resolved expression
/// as a bare, navigable JSX attribute value:
/// - `innerHTML={` — v-html `format!("innerHTML={{{}}}", resolved)`.
/// - `textContent={` — v-text `format!("textContent={{{}}}", resolved)`.
/// - `{[{}]: {}}` — dynamic-key bind `format!("{{...{{[{}]: {}}}}}", arg, value)`
///   (the arg placeholder sits DIRECTLY in the `[ ]`; the v-on dynamic event-name
///   spread instead has a template literal `[`on${}` as any]` there, so this does
///   not match it).
/// - `) = $event)}` — native v-model's baked assignment handler `format!`.
///
/// The wrapped / transformed out-of-scope sites (v-on spreads, eventCallbacks /
/// arrow wrappers, dynamic event-name computed keys, v-show) never emit these
/// substrings inside a `format!` argument to `out.overwrite`.
const DESYNC_FORMAT_SIGNATURES: &[&str] =
    &["innerHTML={", "textContent={", "{[{}]: {}}", ") = $event)}"];

/// Scan a source string for a baked desync mapped-overwrite: an `out.overwrite(`
/// call whose statement (up to the next `;`) contains a `&format!(` replacement
/// carrying one of the four desync bare-JSX-value signatures. Returns the
/// offending statement snippets.
fn baked_prefix_overwrites(src: &str) -> Vec<String> {
    let mut hits = Vec::new();
    let bytes = src.as_bytes();
    let needle = "out.overwrite(";
    let mut search_from = 0usize;
    while let Some(rel) = src[search_from..].find(needle) {
        let start = search_from + rel;
        // Statement extends to the next `;` (the overwrite call terminator).
        let stmt_end = src[start..]
            .find(';')
            .map(|e| start + e)
            .unwrap_or(bytes.len());
        let stmt = &src[start..stmt_end];
        if stmt.contains("&format!(")
            && DESYNC_FORMAT_SIGNATURES
                .iter()
                .any(|sig| stmt.contains(sig))
        {
            hits.push(stmt.split_whitespace().collect::<Vec<_>>().join(" "));
        }
        search_from = stmt_end.max(start + needle.len());
    }
    hits
}

#[test]
fn no_ide_codegen_bakes_prefix_into_mapped_overwrite() {
    let dir = ide_template_dir();
    let mut violations: Vec<(PathBuf, String)> = Vec::new();

    for file in rust_files(&dir) {
        // Skip the codegen tests file: it asserts ABOUT this pattern (negative
        // assertions / fixtures may legitimately mention the strings), and the
        // production guard targets non-test production source.
        if file.file_name().and_then(|n| n.to_str()) == Some("tests.rs") {
            continue;
        }
        let src = std::fs::read_to_string(&file)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", file.display()));
        for stmt in baked_prefix_overwrites(&src) {
            violations.push((file.clone(), stmt));
        }
    }

    assert!(
        violations.is_empty(),
        "IDE codegen bakes a binding prefix + user expression into a single mapped \
         `out.overwrite(.., .., &format!(.. resolved ..))`. This maps the user identifier to the \
         prop start (the go-to-definition desync bug). Emit the expression via the typed `EmitOp` \
         substrate (`emit_jsx_binding_value` / `OverwriteSyntheticBoundary` + `PreserveOriginal`) \
         instead. Violations: {violations:#?}"
    );
}

#[test]
fn baked_prefix_scanner_detects_violation() {
    // Discriminating: the scanner must fire on the exact pre-change shape and on a
    // multi-line variant, and must NOT fire on the typed-substrate replacement.
    let single_line =
        r#"        out.overwrite(prop.start, prop_end, &format!("innerHTML={{{}}}", resolved));"#;
    assert_eq!(
        baked_prefix_overwrites(single_line).len(),
        1,
        "scanner must detect the single-line baked overwrite — else the guard is vacuous"
    );

    let multi_line = "        out.overwrite(\n            prop.start,\n            prop_end,\n            &format!(\"textContent={{{}}}\", resolved),\n        );";
    assert_eq!(
        baked_prefix_overwrites(multi_line).len(),
        1,
        "scanner must detect the multi-line baked overwrite"
    );

    // Each of the four desync signatures must independently trip the scanner —
    // proves no signature is dead.
    let dynamic_key = r#"out.overwrite(prop.start, prop_end, &format!("{{...{{[{}]: {}}}}}", arg_resolved, value_resolved));"#;
    assert_eq!(
        baked_prefix_overwrites(dynamic_key).len(),
        1,
        "scanner must detect the dynamic-key bind baked overwrite"
    );
    let vmodel = r#"out.overwrite(prop.start, prop_end, &format!("{}={{{}}} {}={{({}) => (({}) = $event)}}", dom_prop, resolved, event_name, event_param, resolved));"#;
    assert_eq!(
        baked_prefix_overwrites(vmodel).len(),
        1,
        "scanner must detect the native v-model baked overwrite"
    );

    // The typed-substrate replacement (unmapped boundary + preserved expression)
    // must NOT trip the scanner.
    let clean = r#"        out.overwrite(source.start.0, source.end.0, "");
        out.prepend_static(source.start.0, "innerHTML={");"#;
    assert!(
        baked_prefix_overwrites(clean).is_empty(),
        "scanner false-positived on the clean typed-substrate emission"
    );

    // The out-of-scope v-on dynamic event-name spread must NOT trip the scanner
    // (it is a wrapped computed-key emission, not a bare JSX value).
    let v_on_dynamic = r#"out.overwrite(prop.start, prop_end, &format!("{{...{{[`on${{{}}}` as any]: {}}}}}", resolved_arg, resolved_value));"#;
    assert!(
        baked_prefix_overwrites(v_on_dynamic).is_empty(),
        "scanner false-positived on the out-of-scope v-on dynamic event-name spread"
    );
}

#[test]
fn retired_ide_emit_symbols_absent() {
    let dir = compiler_src_dir();
    let mut found: Vec<(PathBuf, &str)> = Vec::new();

    for file in rust_files(&dir) {
        let src = std::fs::read_to_string(&file)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", file.display()));
        for sym in RETIRED_IDE_EMIT_SYMBOLS {
            if src.contains(sym) {
                found.push((file.clone(), sym));
            }
        }
    }

    assert!(
        found.is_empty(),
        "retired IDE-only flat-string prefixed-expression producer(s) re-introduced in \
         `crates/verter_compiler/src/**`: {found:#?}. These produced flat `prefix + identifier` \
         strings that the desync sites baked into mapped overwrites; they were deleted in favour \
         of the typed `EmitOp` substrate (in-place sites) and the shared `build_prefixed_expr` \
         helper (flat-string consumers). Do not re-add them."
    );
}

#[test]
fn retired_symbol_scanner_detects_presence() {
    // Discriminating: prove the absence check can fail.
    let synthetic = "fn resolve_prefixed_expr(raw: &str) -> String { raw.to_string() }";
    assert!(
        RETIRED_IDE_EMIT_SYMBOLS
            .iter()
            .any(|sym| synthetic.contains(sym)),
        "retired-symbol scanner failed to detect a present producer — the guard would be vacuous"
    );
}
