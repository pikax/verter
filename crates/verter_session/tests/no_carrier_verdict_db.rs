//! Architecture guard: the R22 + R22-fix carrier-verdict +
//! carrier-provenance substrate was removed by Block 6.j R22-final
//! (commit S4). The typed-IR `TypeExpr::SyntheticSlotBinding`
//! variant introduced in S1 fully replaces the old R22 substrate
//! at the projector / registry / reducer surface. Re-introducing
//! any of these symbols outside test fixtures / docs / guards is
//! a regression and must fail this static-grep gate.
//!
//! Discipline mirrors `no_legacy_walker.rs`:
//!
//!  - scans ONLY `crates/*/src/**/*.rs` (production source),
//!  - skips `_tests.rs` / `tests.rs` / files under a `tests/` segment,
//!  - strips line, block, and `#[cfg(test)] mod` modules before
//!    matching (so doc comments and inline tests do not trip the gate),
//!  - matches each symbol at identifier boundaries (so `Carrier`
//!    does NOT match `SyntheticCarrierKey` etc.).
//!
//! Self-exclusion: the first 5 lines of this file contain
//! `R22_CARRIER_GATE_SELF` so the recursive walk skips this file.
//!
//! Per Phase 4 codex Q5 amendment: `PublishedSurfaceKind` is NOT
//! forbidden — `crate::meta_resolve::projection_demand::PublishedSurfaceKind`
//! is a separate, live type that legitimately owns the same
//! identifier. Forbidding the bare token would false-positive on it.

use std::path::{Path, PathBuf};

/// Symbols deleted by Block 6.j R22-final. Any occurrence in
/// `crates/*/src/**` (excluding this guard file and the sibling
/// `architecture_guards.rs`, which carry literal needle strings
/// for their own assertions) is a regression and must fail the
/// gate.
const RETIRED_SYMBOLS: &[&str] = &[
    // Host-owned verdict cache + its identity newtypes.
    "CarrierVerdictDb",
    "CarrierVerdictSlot",
    "CarrierIdentity",
    "CarrierVerdict",
    // Per-component sparse sidecar table + its value type.
    "CarrierProvenance",
    "CarrierProvenanceTable",
    "CarrierValueNodeId",
    // Field name on `ExpandedComponentTypes`.
    "carrier_provenance_table",
    // Accessor on `ProjectTypeStore`.
    "carrier_verdicts",
    // Module path of the retired `crate::carrier_verdict_db` module.
    "carrier_verdict_db",
    // NOTE: `PublishedSurfaceKind` is INTENTIONALLY omitted — the
    // live `crate::meta_resolve::projection_demand::PublishedSurfaceKind`
    // is a different, kept type that legitimately owns the same
    // identifier (Phase 4 codex Q5 amendment).
];

/// File names whose presence at the head of the path should make us
/// self-exclude (this gate file itself plus the sibling
/// `architecture_guards.rs`).
const SELF_EXCLUDED_FILE_NAMES: &[&str] = &["no_carrier_verdict_db.rs", "architecture_guards.rs"];

fn workspace_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    loop {
        if p.join("CLAUDE.md").exists() && p.join("crates").exists() {
            return p;
        }
        if !p.pop() {
            panic!(
                "unable to locate workspace root from {}",
                env!("CARGO_MANIFEST_DIR")
            );
        }
    }
}

/// True for files whose contents are test-only (siblings named
/// `*_tests.rs` or `tests.rs`, or anything inside a `tests/` segment
/// of the path).
fn is_test_file(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    if name == "tests.rs" || name.ends_with("_tests.rs") {
        return true;
    }
    path.components()
        .any(|c| c.as_os_str().to_str() == Some("tests"))
}

fn is_self_excluded(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    SELF_EXCLUDED_FILE_NAMES.contains(&name)
}

/// Walk a `crates/*/src/` tree and collect every `.rs` file that is
/// production source (NOT a test file and NOT self-excluded).
fn collect_production_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name == "target" || name == "node_modules" || name == ".git" {
                continue;
            }
            collect_production_rs(&path, out);
        } else if path.is_file()
            && path.extension().and_then(|e| e.to_str()) == Some("rs")
            && !is_test_file(&path)
            && !is_self_excluded(&path)
        {
            out.push(path);
        }
    }
}

/// Replace `//` line comments and `/* ... */` block comments with
/// equivalent whitespace, preserving newlines so line numbers stay
/// stable. Skips comment-like sequences inside regular and raw
/// string literals.
fn strip_comments(src: &str) -> String {
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
        // Regular string literal "..." with \" escape handling
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

/// Replace the body of every `#[cfg(test)] mod NAME { ... }` block
/// with whitespace (newlines preserved). Inline test modules live
/// inside production source files but are test-only — guard scans
/// must NOT classify them as production violations.
fn strip_inline_test_modules(src: &str) -> String {
    let bytes = src.as_bytes();
    let n = bytes.len();
    let mut out = bytes.to_vec();
    let needle = b"#[cfg(test)]";
    let mut i = 0usize;
    while i + needle.len() <= n {
        if &bytes[i..i + needle.len()] == needle {
            let mut j = i + needle.len();
            let limit = (i + 200).min(n);
            while j < limit {
                if j + 4 <= n && &bytes[j..j + 4] == b"mod " {
                    break;
                }
                j += 1;
            }
            if j + 4 <= n && &bytes[j..j + 4] == b"mod " {
                let mut k = j + 4;
                while k < n && bytes[k] != b'{' && bytes[k] != b';' {
                    k += 1;
                }
                if k < n && bytes[k] == b'{' {
                    let mut depth = 1i32;
                    let mut m = k + 1;
                    while m < n && depth > 0 {
                        match bytes[m] {
                            b'{' => depth += 1,
                            b'}' => depth -= 1,
                            _ => {}
                        }
                        m += 1;
                    }
                    if m > k + 1 {
                        for slot in &mut out[(k + 1)..(m - 1)] {
                            if *slot != b'\n' {
                                *slot = b' ';
                            }
                        }
                    }
                    i = m;
                    continue;
                }
            }
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn preprocess(src: &str) -> String {
    strip_inline_test_modules(&strip_comments(src))
}

/// Identifier-boundary matcher: a retired symbol matches ONLY when
/// its occurrence is bounded by characters that can NOT extend an
/// identifier (i.e., not [A-Za-z0-9_]). This prevents false
/// positives where the retired needle is a prefix of a kept
/// identifier (e.g. forbidding `CarrierVerdict` must NOT match
/// `CarrierVerdictDb` — which is also forbidden, but the
/// individual-needle test for `CarrierVerdictDb` covers that
/// specific case).
fn line_contains_identifier(line: &str, ident: &str) -> bool {
    let bytes = line.as_bytes();
    let needle = ident.as_bytes();
    let n = needle.len();
    if n == 0 || bytes.len() < n {
        return false;
    }
    let is_ident_char = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let mut i = 0usize;
    while i + n <= bytes.len() {
        if &bytes[i..i + n] == needle {
            let before_ok = i == 0 || !is_ident_char(bytes[i - 1]);
            let after_ok = i + n == bytes.len() || !is_ident_char(bytes[i + n]);
            if before_ok && after_ok {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// Collect every production `.rs` file under `crates/*/src/`.
fn collect_production_sources() -> Vec<PathBuf> {
    let root = workspace_root();
    let mut files: Vec<PathBuf> = Vec::new();
    let crates_dir = root.join("crates");
    let Ok(crates) = std::fs::read_dir(&crates_dir) else {
        return files;
    };
    for entry in crates.flatten() {
        let crate_dir = entry.path();
        if !crate_dir.is_dir() {
            continue;
        }
        let src = crate_dir.join("src");
        if !src.is_dir() {
            continue;
        }
        collect_production_rs(&src, &mut files);
    }
    files
}

/// Main gate: every retired R22 symbol must be ABSENT from
/// production source.
#[test]
fn no_carrier_verdict_db_in_production() {
    let files = collect_production_sources();

    for symbol in RETIRED_SYMBOLS {
        let mut hits: Vec<(PathBuf, Vec<usize>)> = Vec::new();
        for file in &files {
            let Ok(text) = std::fs::read_to_string(file) else {
                continue;
            };
            let processed = preprocess(&text);
            let lines: Vec<usize> = processed
                .lines()
                .enumerate()
                .filter_map(|(i, l)| {
                    if line_contains_identifier(l, symbol) {
                        Some(i + 1)
                    } else {
                        None
                    }
                })
                .collect();
            if !lines.is_empty() {
                hits.push((file.clone(), lines));
            }
        }
        assert!(
            hits.is_empty(),
            "retired R22 symbol `{symbol}` reintroduced in production source. \
             Block 6.j R22-final (commit S4) deleted the carrier-verdict + \
             carrier-provenance substrate; the typed-IR \
             `TypeExpr::SyntheticSlotBinding` variant replaces it at the \
             projector / registry / reducer surface. If the symbol is \
             legitimately back in use as a new construct, justify and \
             delete its entry from RETIRED_SYMBOLS.\nHits:\n{hits:#?}"
        );
    }
}

/// Self-test: synthetic-fixture proves the scanner catches a
/// retired symbol. Per Phase 4 improvement 4 + amendment 6 — a
/// guard that does not discriminate is a stub.
#[test]
fn no_carrier_verdict_db_self_test() {
    let synthetic = "pub fn caller() { let _x: CarrierVerdictDb = todo!(); }";
    let processed = preprocess(synthetic);
    let scanner_finds_it = RETIRED_SYMBOLS.iter().any(|sym| {
        processed
            .lines()
            .any(|line| line_contains_identifier(line, sym))
    });
    assert!(
        scanner_finds_it,
        "self-test: the scanner failed to detect a retired R22 \
         symbol in a synthetic production-shape fixture. The \
         RETIRED_SYMBOLS list or the scanner is broken."
    );

    // And: the scanner does NOT trip on a kept identifier that
    // happens to share a prefix with a forbidden one — e.g.
    // `SyntheticCarrierKey` legitimately contains "Carrier" but is
    // a kept type. Identifier-boundary discipline keeps it safe.
    let kept = "use verter_type_expr::SyntheticCarrierKey;";
    let processed_kept = preprocess(kept);
    let scanner_trips_on_kept = RETIRED_SYMBOLS.iter().any(|sym| {
        processed_kept
            .lines()
            .any(|line| line_contains_identifier(line, sym))
    });
    assert!(
        !scanner_trips_on_kept,
        "self-test: the scanner false-positives on `SyntheticCarrierKey` \
         — a KEPT identifier that shares the substring \"Carrier\" with \
         several retired symbols. Identifier-boundary discipline must \
         prevent this."
    );
}

// ---------------------------------------------------------------------
// Architecture guard: explicit deepening of a `SyntheticSlotBinding`
// carrier MUST route through `ShapeCacheKey::semantic_node_whole`.
//
// Contract (per `[[component-meta-shallow-by-default-rule]]` and the
// `TypeExpr::SyntheticSlotBinding` rustdoc):
//   The synthetic carrier variant is a shallow terminal — projectors,
//   reducers, and the registry all refuse to resolve its `binding_name`
//   through `TypeRegistry`. The ONLY legitimate way to deepen a carrier
//   into its underlying member shape is to construct a
//   `ShapeCacheKey::semantic_node_whole(scope, SemanticNodeId(key.value_node), mode)`
//   key and consult `ShapeCacheDb` — the same identity used for any
//   regular member-shape route.
//
//   A consumer that wants to drill down by directly reading
//   `SyntheticCarrierKey::value_node` and wrapping it in a fresh
//   `SemanticNodeId(...)` is BYPASSING the cache route. That is the
//   defect class this guard prevents: such a consumer would (a) miss
//   the shared `ShapeCacheDb` warm-hit path, (b) escape the
//   cache's self-root + dep-signature validation, and (c) drift from
//   the rest of the projector / member-shape pipeline.
//
//   Currently zero workspace consumers exercise the explicit-deepen
//   route — the carrier is always shallow in every projector,
//   reducer, registry, and graph-builder site. The
//   `tests/synthetic_carrier_explicit_deepen_proof.rs` integration
//   test exercises the cache-key identity round-trip via the
//   `ShapeCacheDb::insert_synthetic_carrier_deep_for_test` /
//   `get_synthetic_carrier_deep_for_test` `#[cfg(any(test,
//   debug_assertions))]` helpers, proving the route is well-defined
//   for any future consumer that needs it.
//
//   Narrowing rule (what this guard considers a true bypass):
//     A line that constructs `SemanticNodeId(<ident>.value_node)` is
//     flagged ONLY when no preceding line within a small upstream
//     window (`UPSTREAM_CACHE_ROUTE_WINDOW`) calls
//     `ShapeCacheKey::semantic_node_whole(` (or the
//     `_with_context` variant). When the upstream call is present,
//     the deref is the legitimate cache-route argument site (e.g. an
//     argument expression broken across lines by rustfmt) and is
//     exempt — the consumer IS using the cache route, just split
//     across lines.

/// Identifier-boundary token-stream check: does `line` contain the
/// regex `SemanticNodeId\s*\(\s*([A-Za-z_][A-Za-z0-9_]*)\s*\.\s*value_node`?
///
/// This is the canonical shape of "construct a `SemanticNodeId` from
/// the `value_node` field of some struct value". It catches the
/// suspicious pattern without depending on the binding's name. False
/// positives are possible only if some future unrelated type also
/// names a field `value_node` — in that case the false-positive site
/// must be explicitly allowlisted below.
fn line_constructs_semantic_node_id_from_value_node(line: &str) -> bool {
    let bytes = line.as_bytes();
    let needle = b"SemanticNodeId";
    let is_ident_char = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let mut i = 0usize;
    while i + needle.len() <= bytes.len() {
        if &bytes[i..i + needle.len()] != needle {
            i += 1;
            continue;
        }
        let after_ident = i + needle.len();
        // Identifier-boundary on the right side of `SemanticNodeId` —
        // reject `SemanticNodeIdSomething` (longer identifier).
        if after_ident < bytes.len() && is_ident_char(bytes[after_ident]) {
            i += 1;
            continue;
        }
        // Identifier-boundary on the left side of `SemanticNodeId` —
        // reject `_SemanticNodeId` (e.g. a member-named version).
        if i > 0 && is_ident_char(bytes[i - 1]) {
            i += 1;
            continue;
        }
        // Skip whitespace.
        let mut j = after_ident;
        while j < bytes.len() && bytes[j].is_ascii_whitespace() {
            j += 1;
        }
        // Require an open paren for "constructor call" shape.
        if j >= bytes.len() || bytes[j] != b'(' {
            i += 1;
            continue;
        }
        j += 1;
        while j < bytes.len() && bytes[j].is_ascii_whitespace() {
            j += 1;
        }
        // Match an identifier `[A-Za-z_][A-Za-z0-9_]*`.
        let id_start = j;
        if id_start >= bytes.len()
            || !(bytes[id_start].is_ascii_alphabetic() || bytes[id_start] == b'_')
        {
            i += 1;
            continue;
        }
        let mut id_end = id_start + 1;
        while id_end < bytes.len() && is_ident_char(bytes[id_end]) {
            id_end += 1;
        }
        // Skip whitespace, then expect `.`.
        let mut k = id_end;
        while k < bytes.len() && bytes[k].is_ascii_whitespace() {
            k += 1;
        }
        if k >= bytes.len() || bytes[k] != b'.' {
            i += 1;
            continue;
        }
        k += 1;
        while k < bytes.len() && bytes[k].is_ascii_whitespace() {
            k += 1;
        }
        // Expect literal field name `value_node`.
        let field = b"value_node";
        if k + field.len() > bytes.len() || &bytes[k..k + field.len()] != field {
            i += 1;
            continue;
        }
        // Identifier-boundary on the right side of `value_node` — must
        // not extend into a longer identifier like `value_nodes`.
        let after_field = k + field.len();
        if after_field < bytes.len() && is_ident_char(bytes[after_field]) {
            i += 1;
            continue;
        }
        return true;
    }
    false
}

/// Lines of upstream context the narrowing rule scans for a
/// `ShapeCacheKey::semantic_node_whole(` opening before flagging a
/// `SemanticNodeId(<ident>.value_node)` line as a bypass. Five lines
/// covers the rustfmt-broken legitimate call shape:
///
/// ```ignore
/// let key = ShapeCacheKey::semantic_node_whole(           // -2
///     carrier.scope_canonical_id.clone(),                  // -1
///     crate::semantic_query::SemanticNodeId(carrier.value_node), //  0  ← deref
///     mode,
/// );
/// ```
const UPSTREAM_CACHE_ROUTE_WINDOW: usize = 5;
const CACHE_ROUTE_NEEDLE: &str = "ShapeCacheKey::semantic_node_whole";

/// True if any of the `UPSTREAM_CACHE_ROUTE_WINDOW` lines preceding
/// `idx` contains the cache-route call opener. The check is a plain
/// substring scan on the preprocessed lines (comments + inline
/// `#[cfg(test)] mod` blocks already stripped by the caller). The
/// `_with_context` variant is matched too because its name is a
/// strict superstring of the bare needle.
fn upstream_uses_cache_route(lines: &[&str], idx: usize) -> bool {
    let start = idx.saturating_sub(UPSTREAM_CACHE_ROUTE_WINDOW);
    lines[start..idx]
        .iter()
        .any(|l| l.contains(CACHE_ROUTE_NEEDLE))
}

/// The architecture guard test: no production source file may
/// construct a `SemanticNodeId` from any struct's `value_node` field
/// EXCEPT when the construction is inside the argument list of a
/// `ShapeCacheKey::semantic_node_whole(...)` (or
/// `_with_context(...)`) call. The cache-route call is the only
/// permitted way to express a synthetic-carrier-derived semantic-node
/// identity; rustfmt may split the call's argument across lines, so
/// the upstream-window check accepts the legitimate split shape.
#[test]
fn synthetic_carrier_explicit_deepen_routes_through_shape_cache_key() {
    let files = collect_production_sources();

    let mut violations: Vec<(PathBuf, Vec<usize>)> = Vec::new();
    for file in &files {
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        let processed = preprocess(&text);
        let processed_lines: Vec<&str> = processed.lines().collect();
        let violation_lines: Vec<usize> = processed_lines
            .iter()
            .enumerate()
            .filter_map(|(i, l)| {
                if !line_constructs_semantic_node_id_from_value_node(l) {
                    return None;
                }
                // Narrowing: if the upstream window of this line
                // contains the cache-route call opener, the deref is
                // the legitimate cache-route argument (split across
                // lines by rustfmt). Exempt it.
                if upstream_uses_cache_route(&processed_lines, i) {
                    return None;
                }
                Some(i + 1)
            })
            .collect();
        if !violation_lines.is_empty() {
            violations.push((file.clone(), violation_lines));
        }
    }

    assert!(
        violations.is_empty(),
        "Architecture rule violation: production source constructs a \
         `SemanticNodeId` directly from a struct's `value_node` field \
         outside the legitimate cache-route call. The synthetic-carrier \
         explicit-deepen route MUST go through \
         `ShapeCacheKey::semantic_node_whole(scope, node, mode)` (and \
         its `_with_context` variant). Direct `SemanticNodeId(_.value_node)` \
         construction outside that call bypasses the shared \
         `ShapeCacheDb` warm-hit path AND escapes self-root + \
         dep-signature validation. See \
         `[[component-meta-shallow-by-default-rule]]` in \
         `.claude/skills/component-meta/SKILL.md`.\n\nViolations:\n{violations:#?}"
    );
}

/// Self-test for the explicit-deepen guard: the per-line scanner
/// catches the forbidden shape AND the narrowed end-to-end guard
/// (upstream-window check) discriminates legitimate cache-route
/// argument sites from bare bypasses.
#[test]
fn synthetic_carrier_explicit_deepen_guard_self_test() {
    // -----------------------------------------------------------------
    // Part 1 — per-line scanner: forbidden shapes match, legitimate
    // shapes do not. (No upstream context yet — that lives in Part 2.)
    // -----------------------------------------------------------------

    // The forbidden shape — constructs `SemanticNodeId` from any
    // struct's `value_node` field.
    let violation_a = "let id = SemanticNodeId(key.value_node);";
    let violation_b = "    SemanticNodeId(carrier.value_node)";
    let violation_c = "SemanticNodeId ( binding . value_node ) // whitespace tolerance";

    assert!(
        line_constructs_semantic_node_id_from_value_node(violation_a),
        "self-test: scanner missed `SemanticNodeId(key.value_node);` — \
         the canonical shape of a forbidden direct construction"
    );
    assert!(
        line_constructs_semantic_node_id_from_value_node(violation_b),
        "self-test: scanner missed `SemanticNodeId(carrier.value_node)`"
    );
    assert!(
        line_constructs_semantic_node_id_from_value_node(violation_c),
        "self-test: scanner missed whitespace-tolerant `SemanticNodeId ( binding . value_node )`"
    );

    // Per-line legitimate shapes — must NOT trip the scanner.
    let legit_cache_route_pre_bound =
        "let key = ShapeCacheKey::semantic_node_whole(scope, member_value, mode);";
    let legit_field_decl = "pub value_node: u64,";
    let legit_struct_constr =
        "SyntheticCarrierKey { scope_canonical_id: s, surface_kind: k, slot_name: n, binding_name: b, value_node: 7 }";
    let legit_serialize = "key.value_node.to_string()";
    let legit_semantic_node_id_from_const = "let id = SemanticNodeId(0);";
    let legit_semantic_node_id_from_field_no_value_node =
        "let id = SemanticNodeId(member.id_field);";

    for legit in [
        legit_cache_route_pre_bound,
        legit_field_decl,
        legit_struct_constr,
        legit_serialize,
        legit_semantic_node_id_from_const,
        legit_semantic_node_id_from_field_no_value_node,
    ] {
        assert!(
            !line_constructs_semantic_node_id_from_value_node(legit),
            "self-test: scanner false-positives on a legitimate line: `{legit}`"
        );
    }

    // -----------------------------------------------------------------
    // Part 2 — narrowing: the per-line scanner DOES match the deref
    // shape, but the upstream-window check exempts it when the
    // legitimate cache-route call opener appears in the upstream
    // window. This is the rustfmt-broken legitimate shape codex
    // flagged as previously banned.
    // -----------------------------------------------------------------

    let legit_split_call: &[&str] = &[
        "let key = ShapeCacheKey::semantic_node_whole(",
        "    carrier.scope_canonical_id.clone(),",
        "    crate::semantic_query::SemanticNodeId(carrier.value_node),",
        "    mode,",
        ");",
    ];
    // The deref line on its own DOES match the per-line scanner.
    assert!(
        line_constructs_semantic_node_id_from_value_node(legit_split_call[2]),
        "self-test: per-line scanner missed the deref inside the \
         rustfmt-broken legitimate call"
    );
    // But the upstream-window check exempts it.
    assert!(
        upstream_uses_cache_route(legit_split_call, 2),
        "self-test: upstream-window check failed to find \
         `ShapeCacheKey::semantic_node_whole(` 2 lines upstream of the \
         deref. The legitimate cache-route argument shape would be \
         false-flagged."
    );

    // The `_with_context` variant must also be recognised as legit
    // because its name is a strict superstring of the bare needle.
    let legit_split_call_with_ctx: &[&str] = &[
        "let key = ShapeCacheKey::semantic_node_whole_with_context(",
        "    carrier.scope_canonical_id.clone(),",
        "    crate::semantic_query::SemanticNodeId(carrier.value_node),",
        "    terminal_context,",
        ");",
    ];
    assert!(
        upstream_uses_cache_route(legit_split_call_with_ctx, 2),
        "self-test: upstream-window check failed to recognise \
         `_with_context` variant of the cache-route call"
    );

    // -----------------------------------------------------------------
    // Part 3 — narrowing discriminates: a bare bypass (no cache-route
    // call in the upstream window) IS flagged. This is the defect
    // class the guard exists to prevent.
    // -----------------------------------------------------------------

    let bypass_bare: &[&str] = &[
        "fn deepen_directly(carrier: &SyntheticCarrierKey) {",
        "    let n = SemanticNodeId(carrier.value_node);",
        "    do_something_else(n);",
        "}",
    ];
    // The deref line matches the per-line scanner.
    assert!(
        line_constructs_semantic_node_id_from_value_node(bypass_bare[1]),
        "self-test: per-line scanner missed the bare bypass deref"
    );
    // And the upstream-window check does NOT exempt it (no cache-route
    // call opener in upstream).
    assert!(
        !upstream_uses_cache_route(bypass_bare, 1),
        "self-test: upstream-window check false-positively found a \
         cache-route call in upstream context of a bare bypass — the \
         guard would silently permit the defect class it exists to \
         prevent"
    );

    // A bypass that happens to mention the cache-route name in an
    // unrelated string literal further upstream than the window is
    // STILL flagged — the window must be bounded.
    let bypass_distant_mention: &[&str] = &[
        "// older code referenced ShapeCacheKey::semantic_node_whole here",
        "fn pad1() {}",
        "fn pad2() {}",
        "fn pad3() {}",
        "fn pad4() {}",
        "fn pad5() {}",
        "fn deepen_directly(carrier: &SyntheticCarrierKey) {",
        "    let n = SemanticNodeId(carrier.value_node);",
        "}",
    ];
    assert!(
        line_constructs_semantic_node_id_from_value_node(bypass_distant_mention[7]),
        "self-test: per-line scanner missed the deref in the distant-mention bypass"
    );
    assert!(
        !upstream_uses_cache_route(bypass_distant_mention, 7),
        "self-test: upstream window must be bounded; the cache-route \
         mention 7 lines back must NOT exempt the bypass"
    );
}
