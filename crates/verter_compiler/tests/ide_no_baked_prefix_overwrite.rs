//! IDE-emit architecture guard: no navigable user expression baked into a mapped
//! `out.overwrite`.
//!
//! A go-to-definition desync occurs when IDE codegen collapses a binding-resolved
//! user expression into ONE `out.overwrite(start, end, &format!("…{resolved}…"))`.
//! The resulting `Chunk::Overwritten` maps its whole generated run back to the
//! overwrite's source start — so the user identifier maps to a FOREIGN position
//! (the prop start), and under Phase-1's strict mapper the identifier drops to
//! `None`. Either way the identifier is no longer navigable. Every navigable user
//! expression MUST instead flow through the typed `EmitOp` substrate
//! (`crates/verter_compiler/src/ide/template/emit.rs`): the user expression is
//! emitted via `emit_jsx_binding_value` / `emit_relocated_value` (each identifier a
//! mapped `InsertMapped` / preserved `Original`), and all synthetic scaffolding is
//! an unmapped `InsertUnmapped` / `OverwriteSyntheticBoundary`. This guard pins
//! that invariant mechanically — it CATCHES the bug class rather than exempting it.
//!
//! Two checks:
//! 1. `no_ide_codegen_bakes_prefix_into_mapped_overwrite` — source scan over
//!    `crates/verter_compiler/src/ide/template/**` forbidding ANY `out.overwrite(…)`
//!    whose `&format!` replacement interpolates a binding-resolved expression
//!    variable (`resolved` / `resolved_expr` / `resolved_arg` / `resolved_value` /
//!    `final_expr` / `resolved_style`). The allowlist
//!    (`ALLOWED_NON_NAVIGABLE_OVERWRITES`) is EMPTY: every navigable user
//!    expression routes through the substrate. v-on spreads, the dynamic
//!    event-name spread, native + dynamic v-model, v-show, AND the no-value
//!    `:foo` / `.foo` shorthands (whose value `foo` ≡ `:foo="foo"` is navigable)
//!    all emit through the substrate and have ZERO baked navigable overwrites.
//! 2. `retired_ide_emit_symbols_absent` — the flat-string producers
//!    `resolve_prefixed_expr` / `resolve_prefixed_dynamic_arg` must be ABSENT from
//!    `crates/verter_compiler/src/**` (they were deleted; a lingering producer
//!    would re-open the desync path).
//!
//! Each check is paired with a discriminating sanity test proving the scanner
//! actually fires on a synthetic violation (so the guard is not vacuous).

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

/// Binding-resolver output variable names. A `&format!` argument to `out.overwrite`
/// that interpolates one of these is baking a binding-resolved user expression into
/// a single `Chunk::Overwritten` — the desync shape this guard forbids. (These are
/// the conventional names the IDE prop emitters use for `build_prefixed_expr` /
/// `resolve_simple_expr` output.)
const RESOLVER_OUTPUT_VARS: &[&str] = &[
    "resolved",
    "resolved_expr",
    "resolved_arg",
    "resolved_value",
    "resolved_style",
    "final_expr",
    // `rewritten` is `rewrite_v_on_object_literal_expr(&resolved)` — the v-on
    // object-literal spread's structurally-rewritten resolved expression. Baking it
    // into a mapped overwrite is the same desync (it embeds the handler identifiers).
    "rewritten",
];

/// Normalised (whitespace-collapsed) statement snippets that are EXPLICITLY allowed
/// to bake a resolved expression into a mapped overwrite because they are PROVABLY
/// non-navigable (no in-place user value identifier to map).
///
/// This allowlist is EMPTY: every navigable user expression in `ide/template/**`
/// — including the no-value `:foo` / `.foo` shorthands, whose value `foo` ≡
/// `:foo="foo"` IS a navigable binding-resolved identifier — routes through the
/// typed `EmitOp` substrate. The earlier entries that exempted those two shorthand
/// sites were a gate-bypass: they baked the resolved VALUE identifier (`$setup.foo`
/// / `foo.value`) into a mapped overwrite, mapping it to a foreign position so
/// ctrl+click failed. They are now emitted via `emit_synthesized_shorthand_value`
/// (the value core a mapped `InsertMapped` pointing at the arg/key source token,
/// the accessor prefix/suffix unmapped) and removed from this allowlist.
///
/// Keyed by exact normalised statement text (not a fuzzy match) so any NEW baked
/// overwrite trips the guard and must be re-justified before it can be added here.
const ALLOWED_NON_NAVIGABLE_OVERWRITES: &[&str] = &[];

/// Whitespace-collapse a statement snippet for stable comparison/allowlisting.
fn normalize_stmt(stmt: &str) -> String {
    stmt.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `true` iff the `&format!` replacement in `stmt` interpolates a binding-resolver
/// output variable as a `{}` argument. Detects both `&format!("…", resolved)` and
/// `&format!("…{resolved}…")` (inline-captured) forms.
fn format_interpolates_resolver_var(stmt: &str) -> bool {
    let Some(fmt_pos) = stmt.find("&format!(") else {
        return false;
    };
    let args = &stmt[fmt_pos..];
    RESOLVER_OUTPUT_VARS.iter().any(|var| {
        // Match the variable as a whole token: preceded by a non-ident char and
        // followed by a non-ident char (avoids matching `resolved` inside
        // `resolved_expr`, and `final_expr` inside a longer ident).
        token_present(args, var)
    })
}

/// `true` iff `needle` appears in `hay` as a whole identifier token (not a substring
/// of a longer identifier).
fn token_present(hay: &str, needle: &str) -> bool {
    let mut from = 0usize;
    while let Some(rel) = hay[from..].find(needle) {
        let at = from + rel;
        let before_ok = at == 0
            || !hay.as_bytes()[at - 1].is_ascii_alphanumeric() && hay.as_bytes()[at - 1] != b'_';
        let after = at + needle.len();
        let after_ok = after >= hay.len()
            || !hay.as_bytes()[after].is_ascii_alphanumeric() && hay.as_bytes()[after] != b'_';
        if before_ok && after_ok {
            return true;
        }
        from = at + needle.len();
    }
    false
}

/// Scan a source string for a baked desync mapped-overwrite: an `out.overwrite(`
/// call whose statement (up to the next `;`) bakes a binding-resolved expression
/// into a `&format!` replacement, EXCLUDING the explicitly-allowed
/// provably-non-navigable sites. Returns the offending normalised statement
/// snippets.
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
            .map(|e| start + e + 1)
            .unwrap_or(bytes.len());
        let stmt = &src[start..stmt_end];
        if format_interpolates_resolver_var(stmt) {
            let norm = normalize_stmt(stmt);
            if !ALLOWED_NON_NAVIGABLE_OVERWRITES.contains(&norm.as_str()) {
                hits.push(norm);
            }
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
    // Discriminating: the scanner must fire on every baked-resolved-expression
    // overwrite shape (single-line, multi-line) and must NOT fire on the
    // typed-substrate replacement nor the allowlisted no-value attributes.
    let single_line =
        r#"        out.overwrite(prop.start, prop_end, &format!("innerHTML={{{}}}", resolved));"#;
    assert_eq!(
        baked_prefix_overwrites(single_line).len(),
        1,
        "scanner must detect the single-line baked overwrite — else the guard is vacuous"
    );

    let multi_line = "        out.overwrite(\n            prop.start,\n            prop_end,\n            &format!(\"textContent={{{}}}\", resolved_expr),\n        );";
    assert_eq!(
        baked_prefix_overwrites(multi_line).len(),
        1,
        "scanner must detect the multi-line baked overwrite"
    );

    // Native v-model's baked assignment handler must trip the scanner.
    let vmodel = r#"out.overwrite(prop.start, prop_end, &format!("{}={{{}}} {}={{({}) => (({}) = $event)}}", dom_prop, resolved, event_name, event_param, resolved));"#;
    assert_eq!(
        baked_prefix_overwrites(vmodel).len(),
        1,
        "scanner must detect the native v-model baked overwrite"
    );

    // CRITICAL (FINDING 3): the v-on baked handler overwrites — object spread,
    // dynamic event-name spread, and the duplicate-event spread — MUST now be
    // DETECTED. The previous guard EXEMPTED these (a gate-bypass). Each is a
    // navigable handler baked into a mapped overwrite → the desync bug.
    // The exact pre-fix object-literal shape baked `rewritten`
    // (= `rewrite_v_on_object_literal_expr(&resolved)`) into the overwrite.
    let v_on_object = r#"out.overwrite(prop.start, prop_end, &format!("{{...{}}}", rewritten));"#;
    assert_eq!(
        baked_prefix_overwrites(v_on_object).len(),
        1,
        "scanner MUST detect the v-on object-literal baked `rewritten` spread (was exempted — gate-bypass)"
    );
    let v_on_dynamic = r#"out.overwrite(prop.start, prop_end, &format!("{{...{{[`on${{{}}}` as any]: {}}}}}", resolved_arg, resolved_value));"#;
    assert_eq!(
        baked_prefix_overwrites(v_on_dynamic).len(),
        1,
        "scanner MUST detect the v-on dynamic event-name baked spread (was exempted — gate-bypass)"
    );
    let v_on_spread = r#"out.overwrite(prop.start, prop_end, &format!("{{...{{\"{}\": {}}}}}", jsx_event_name, resolved_expr));"#;
    assert_eq!(
        baked_prefix_overwrites(v_on_spread).len(),
        1,
        "scanner MUST detect the v-on duplicate-event baked spread (was exempted — gate-bypass)"
    );
    // v-show's baked display style must also be detected.
    let v_show = r#"out.overwrite(show.start, prop_end, &format!("style={{{{display: {} ? undefined : 'none'}}}}", resolved_expr));"#;
    assert_eq!(
        baked_prefix_overwrites(v_show).len(),
        1,
        "scanner MUST detect the v-show baked display-style overwrite"
    );

    // The typed-substrate replacement (unmapped boundary / mapped relocated value)
    // must NOT trip the scanner — it has no `&format!`-baked resolved expression.
    let clean = r#"        out.overwrite(source.start.0, source.end.0, "");
        out.prepend_static(source.start.0, "innerHTML={");"#;
    assert!(
        baked_prefix_overwrites(clean).is_empty(),
        "scanner false-positived on the clean typed-substrate emission"
    );
    let clean_relocated = r#"            out.overwrite(prop.start, prop_end, "");
            emit_relocated_value(out, at, source, value_range, value_bindings, resolver);"#;
    assert!(
        baked_prefix_overwrites(clean_relocated).is_empty(),
        "scanner false-positived on the clean emit_relocated_value substrate path"
    );

    // The allowlist is EMPTY — nothing is exempt.
    assert!(
        ALLOWED_NON_NAVIGABLE_OVERWRITES.is_empty(),
        "the baked-overwrite allowlist must stay empty: every navigable user expression routes \
         through the EmitOp substrate. A new entry re-opens the desync gate-bypass."
    );

    // The two FORMERLY-allowlisted no-value shorthand bakes (`:foo` / `.foo`) were a
    // gate-bypass: the resolved VALUE identifier (`$setup.foo` / `foo.value`) IS
    // navigable. The scanner MUST now DETECT both shapes — they have been migrated to
    // the EmitOp substrate, and re-introducing either flat-string bake must trip.
    let v_bind_shorthand_no_value =
        r#"out.overwrite(arg_end, prop_end, &format!("={{{}}}", resolved));"#;
    assert_eq!(
        baked_prefix_overwrites(v_bind_shorthand_no_value).len(),
        1,
        "scanner MUST detect the `:foo` no-value shorthand baked value (was allowlisted — gate-bypass)"
    );
    let dot_prop_shorthand_no_value =
        r#"out.overwrite(prop.start, prop_end, &format!("{}={{{}}}", key, resolved));"#;
    assert_eq!(
        baked_prefix_overwrites(dot_prop_shorthand_no_value).len(),
        1,
        "scanner MUST detect the `.foo` no-value shorthand baked value (was allowlisted — gate-bypass)"
    );

    // The token matcher must not match a resolver var as a substring of a longer
    // identifier (e.g. `resolved` inside `resolved_handler_name`).
    let longer_ident =
        r#"out.overwrite(prop.start, prop_end, &format!("{}", resolved_handler_name));"#;
    assert!(
        baked_prefix_overwrites(longer_ident).is_empty(),
        "scanner must match resolver vars as whole tokens, not substrings of longer idents"
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
