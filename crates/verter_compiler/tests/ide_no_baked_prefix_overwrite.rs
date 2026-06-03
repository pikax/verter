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
//!    `final_expr` / `resolved_style`), AND any `let`-indirected bake
//!    (`let v = …resolver-output…; out.overwrite(.., &v)`). The check is purely
//!    structural on RHS PROVENANCE — there is NO self-anchored-span exclusion (span
//!    shape does not prove the baked text is the node's own resolved form). The
//!    allowlist (`ALLOWED_NON_NAVIGABLE_OVERWRITES`) is EMPTY: every navigable user
//!    expression routes through the unified `plan_user_expr` planner / the
//!    `emit_synthesized_shorthand_value` substrate. v-on spreads + the non-object
//!    `v-on="obj"` spread, the dynamic event-name spread, native + dynamic v-model,
//!    v-show, the no-value `:foo` / `.foo` shorthands, AND the broken-interpolation
//!    keyword-member recovery (`SynthesizedResolved`) all emit through the
//!    substrate and have ZERO baked navigable overwrites.
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
    // A structurally-rewritten resolved object-literal expression (the conventional
    // `rewritten` name): baking it into a mapped overwrite embeds the handler
    // identifiers and is the same desync. Kept as a defensive forbidden name.
    "rewritten",
];

/// Resolver-output PRODUCER functions. A `let VAR = build_prefixed_expr(...)`
/// (etc.) binds a binding-resolved expression string to `VAR`; baking `VAR` into a
/// mapped overwrite is the same desync as baking the inline `&format!(...resolved...)`.
/// These are the shared producers the IDE prop emitters call to obtain a flat
/// resolved expression.
const RESOLVER_OUTPUT_PRODUCERS: &[&str] = &[
    "build_prefixed_expr",
    "resolve_simple_expr",
    "resolve_prefixed_expr",
    "resolve_prefixed_dynamic_arg",
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

/// `true` iff `rhs` (the right-hand side of a `let` binding, up to its terminating
/// `;`) yields a binding-resolved expression: either it interpolates a
/// resolver-output variable inside a `format!`, or it calls a resolver-output
/// PRODUCER (`build_prefixed_expr`, `resolve_simple_expr`, …). Such a value is a
/// navigable user expression in flat-string form — baking it into a mapped
/// overwrite is the desync this guard forbids.
fn rhs_is_resolver_output(rhs: &str) -> bool {
    // A `format!(…)` interpolating a resolver-output var (e.g. `final_expr`).
    if let Some(fmt_pos) = rhs.find("format!(") {
        let args = &rhs[fmt_pos..];
        if RESOLVER_OUTPUT_VARS
            .iter()
            .any(|var| token_present(args, var))
        {
            return true;
        }
    }
    // A direct call to a resolver-output producer.
    RESOLVER_OUTPUT_PRODUCERS
        .iter()
        .any(|producer| token_present(rhs, producer))
}

/// Collect the names of local variables bound (via `let <ident> = …;`) to a
/// resolver-output value within `src`. These are the "tainted" vars whose later
/// use as an `out.overwrite` replacement re-opens the baked-overwrite desync
/// through a `let`-indirection (the inline-`&format!` check alone misses it).
///
/// Structural, not fuzzy: matches the `let [mut] <ident> = <rhs>;` binding form and
/// classifies `<rhs>` via [`rhs_is_resolver_output`]. (`<rhs>` extends to the next
/// `;`, which over-approximates for multi-statement-on-one-line code but the IDE
/// emitters use one binding per statement.)
fn resolver_output_bound_vars(src: &str) -> Vec<String> {
    let mut vars = Vec::new();
    let mut from = 0usize;
    while let Some(rel) = src[from..].find("let ") {
        let at = from + rel;
        // `let ` must start a token (preceded by a non-ident char) to avoid matching
        // inside an identifier like `complete_set`.
        let preceded_ok = at == 0 || {
            let b = src.as_bytes()[at - 1];
            !b.is_ascii_alphanumeric() && b != b'_'
        };
        let after_let = at + "let ".len();
        if !preceded_ok {
            from = after_let;
            continue;
        }
        // Skip an optional `mut `.
        let mut name_start = after_let;
        if src[name_start..].starts_with("mut ") {
            name_start += "mut ".len();
        }
        // The binding name runs until a non-ident char.
        let name_end = src[name_start..]
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .map(|i| name_start + i)
            .unwrap_or(src.len());
        let name = &src[name_start..name_end];
        // Require a `=` (not `==`) before the next `;` to be a binding with an RHS.
        let rest = &src[name_end..];
        let stmt_end = rest.find(';').map(|e| e + 1).unwrap_or(rest.len());
        let stmt = &rest[..stmt_end];
        if !name.is_empty() {
            if let Some(eq) = stmt.find('=') {
                let is_assignment = stmt.as_bytes().get(eq + 1) != Some(&b'=');
                if is_assignment {
                    let rhs = &stmt[eq + 1..];
                    if rhs_is_resolver_output(rhs) {
                        vars.push(name.to_string());
                    }
                }
            }
        }
        from = name_end;
    }
    vars
}

/// Scan `src` for a baked desync mapped-overwrite reached via a `let`-indirection:
/// an `out.overwrite(start, end, &VAR)` whose `&VAR` references a variable bound to
/// a resolver-output value (collected by [`resolver_output_bound_vars`]).
///
/// There is NO self-anchored exclusion: span shape (`out.overwrite(base + n.start,
/// base + n.end, &v)`) does NOT prove the replacement is the node's own resolved
/// form — `&v` can be ANY resolver-output string baked over an unrelated node's
/// span. A mapped overwrite that bakes resolver output ALWAYS maps its whole
/// generated run to the overwrite start (the desync), so it is flagged regardless of
/// whether `start`/`end` are the same node's endpoints. The navigable in-place
/// emission must instead route through the typed `EmitOp` substrate / the unified
/// `emit_synthesized_shorthand_value` (each surviving identifier a mapped insertion;
/// synthetic scaffolding unmapped). Returns the offending normalised statements.
fn indirected_baked_overwrites(src: &str) -> Vec<String> {
    let tainted = resolver_output_bound_vars(src);
    if tainted.is_empty() {
        return Vec::new();
    }
    let mut hits = Vec::new();
    let needle = "out.overwrite(";
    let mut search_from = 0usize;
    while let Some(rel) = src[search_from..].find(needle) {
        let start = search_from + rel;
        let stmt_end = src[start..]
            .find(';')
            .map(|e| start + e + 1)
            .unwrap_or(src.len());
        let stmt = &src[start..stmt_end];
        // The replacement argument is `&VAR` for some tainted VAR. Match `&VAR` as a
        // whole reference token (the `&` immediately precedes the ident, and the
        // ident is whole). This avoids matching `&format!(... VAR ...)` (handled by
        // the inline check) and substrings of longer idents.
        let flagged = tainted.iter().any(|var| {
            let pat = format!("&{var}");
            let mut scan = 0usize;
            while let Some(r) = stmt[scan..].find(&pat) {
                let p = scan + r;
                let after = p + pat.len();
                let after_ok = after >= stmt.len() || {
                    let b = stmt.as_bytes()[after];
                    !b.is_ascii_alphanumeric() && b != b'_'
                };
                if after_ok {
                    return true;
                }
                scan = p + pat.len();
            }
            false
        });
        if flagged {
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
        // Inline form: `out.overwrite(.., &format!(.. resolved ..))`.
        for stmt in baked_prefix_overwrites(&src) {
            violations.push((file.clone(), stmt));
        }
        // `let`-indirected form: `let v = …format!(..resolved..)…; out.overwrite(.., &v)`.
        for stmt in indirected_baked_overwrites(&src) {
            violations.push((file.clone(), stmt));
        }
    }

    assert!(
        violations.is_empty(),
        "IDE codegen bakes a binding prefix + user expression into a single mapped \
         `out.overwrite(.., .., &format!(.. resolved ..))` (inline) OR via a \
         `let v = …format!(.. resolved ..)…; out.overwrite(.., &v)` indirection. This maps the \
         user identifier to the overwrite start (the go-to-definition desync bug). Emit the \
         expression via the typed `EmitOp` substrate (`emit_jsx_binding_value` / \
         `OverwriteSyntheticBoundary` + `PreserveOriginal`, or an in-place boundary split + \
         `collect_binding_patches` with the synthetic guard as an unmapped prepend) instead. \
         Violations: {violations:#?}"
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
fn indirected_baked_overwrite_scanner_detects_violation() {
    // Discriminating: the strengthened scanner must FIRE on the `let`-indirected
    // baked overwrite (the FINDING-A guard hole) and must NOT fire on the clean
    // typed-substrate emission.

    // 1. `let close = format!(… final_expr …); out.overwrite(.., .., &close);` —
    //    the exact pre-fix props.rs `guarded.is_some()` shape. `final_expr` is a
    //    resolver-output var, so `close` is tainted, so the overwrite is flagged.
    let let_format_final_expr = r#"
        let final_expr = guarded.as_deref().unwrap_or(&resolved);
        let close = format!("={{{}}}", final_expr);
        out.overwrite(arg_end, prop_end, &close);
    "#;
    assert_eq!(
        indirected_baked_overwrites(let_format_final_expr).len(),
        1,
        "scanner MUST detect `let close = format!(.. final_expr ..); out.overwrite(.., &close)` — \
         else the guard hole (FINDING A) stays open"
    );

    // 2. `let close = format!(… resolved …); out.overwrite(.., .., &close);` — the
    //    generic resolver-var indirection.
    let let_format_resolved = r#"
        let close = format!("={{{}}}", resolved);
        out.overwrite(arg_end, prop_end, &close);
    "#;
    assert_eq!(
        indirected_baked_overwrites(let_format_resolved).len(),
        1,
        "scanner MUST detect the `let close = format!(.. resolved ..); overwrite(.., &close)` indirection"
    );

    // 3. A var bound DIRECTLY to a resolver-output PRODUCER, then baked at a FOREIGN
    //    anchor: `let v = build_prefixed_expr(...); out.overwrite(arg_end, prop_end,
    //    &v);`. Producer-bound taint is in scope (codex: the absence of `format!`
    //    does not make it safe) → flagged.
    let let_producer_foreign = r#"
        let v = build_prefixed_expr(value_expr, vs, exp, resolver, &[]);
        out.overwrite(arg_end, prop_end, &v);
    "#;
    assert_eq!(
        indirected_baked_overwrites(let_producer_foreign).len(),
        1,
        "scanner MUST detect a producer-bound var baked into an overwrite \
         (the future-pattern desync codex flagged)"
    );

    // 4. CLEAN: the migrated in-place substrate — boundary split + unmapped guard
    //    prepend + `collect_binding_patches`. The guard text is computed via a
    //    resolver-output producer (`build_block_guard` is NOT a producer), the value
    //    stays in place, and NO tainted var is baked into an overwrite. Must NOT fire.
    let clean_substrate = r#"
        out.overwrite(prop.start, arg_start, "");
        out.overwrite(arg_end, tvs, "={");
        out.overwrite(tve, prop_end, close_brace);
        out.prepend_alloc(injection.source_offset, &injection.text);
        resolver.collect_binding_patches(bindings, out);
    "#;
    assert!(
        indirected_baked_overwrites(clean_substrate).is_empty(),
        "scanner false-positived on the clean in-place substrate (guard as unmapped prepend, \
         value preserved in place)"
    );

    // 5. A SELF-ANCHORED bake — `let v = build_prefixed_expr(value); out.overwrite(
    //    node.start, node.end, &v)` — MUST now FIRE. There is no self-anchored
    //    exclusion: span shape does not prove the baked text is the node's own
    //    resolved form, and a mapped overwrite of resolver output ALWAYS maps its
    //    whole run to the overwrite start (the desync). This is the Q2 guard
    //    strengthening — the previously-exempted self-anchored shape is the very bug
    //    class the broken-interpolation recovery (`SynthesizedResolved`) was migrated
    //    off of.
    let self_anchored_bake = r#"
        let resolved = resolver.resolve_simple_expr(ident);
        out.overwrite(
            expr_start + recovered_ident.start as u32,
            expr_start + recovered_ident.end as u32,
            &resolved,
        );
    "#;
    assert_eq!(
        indirected_baked_overwrites(self_anchored_bake).len(),
        1,
        "scanner MUST detect a SELF-ANCHORED resolver-output bake (`out.overwrite(node.start, \
         node.end, &resolved)`) — the self-anchored exclusion was REMOVED in Q2 because span \
         shape does not prove RHS provenance"
    );

    // 5b. The migrated recovery (`emit_synthesized_shorthand_value` — delete + mapped
    //     core + unmapped scaffolding) is NOT a bake and must NOT fire.
    let clean_synthesized_recover = r#"
        let resolved = resolver.resolve_simple_expr(ident);
        out.overwrite(ident_start, expr_start + recovered_ident.end as u32, "");
        emit_synthesized_shorthand_value(
            out,
            SourceByteOffset(ident_start),
            &resolved,
            &ident,
            SourceByteOffset(ident_start),
        );
    "#;
    assert!(
        indirected_baked_overwrites(clean_synthesized_recover).is_empty(),
        "scanner false-positived on the migrated synthesized-recovery emission (the overwrite \
         deletes the span with \"\" and the resolved form is emitted via the unified substrate, \
         not baked into a mapped overwrite)"
    );

    // 6. The token matcher must not flag `&close_handler` when only `close` is tainted.
    let longer_ident = r#"
        let close = format!("={{{}}}", resolved);
        out.overwrite(arg_end, prop_end, &close_handler);
    "#;
    assert!(
        indirected_baked_overwrites(longer_ident).is_empty(),
        "scanner must match the baked `&VAR` reference as a whole token, not a substring of a \
         longer identifier"
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
