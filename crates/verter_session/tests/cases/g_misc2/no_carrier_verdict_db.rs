//! Architecture guard: the R22 carrier-verdict + carrier-provenance
//! substrate was removed. The typed-IR `TypeExpr::SyntheticSlotBinding`
//! variant fully replaces the old R22 substrate at the projector /
//! registry / reducer surface. Re-introducing any of these symbols
//! outside test fixtures / docs / guards is a regression and must fail
//! this static-grep gate.
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
//! `PublishedSurfaceKind` is NOT forbidden —
//! `crate::meta_resolve::projection_demand::PublishedSurfaceKind`
//! is a separate, live type that legitimately owns the same
//! identifier. Forbidding the bare token would false-positive on it.

use std::path::{Path, PathBuf};

/// Symbols deleted with the R22 carrier substrate. Any occurrence in
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
    // identifier.
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

    // Read + preprocess each file ONCE, then test every retired symbol against
    // the cached processed text (was O(symbols × files) of redundant reads).
    let mut hits_by_symbol: std::collections::BTreeMap<&str, Vec<(PathBuf, Vec<usize>)>> =
        RETIRED_SYMBOLS.iter().map(|s| (*s, Vec::new())).collect();
    for file in &files {
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        let processed = preprocess(&text);
        let plines: Vec<&str> = processed.lines().collect();
        for symbol in RETIRED_SYMBOLS {
            let lines: Vec<usize> = plines
                .iter()
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
                hits_by_symbol
                    .get_mut(symbol)
                    .expect("symbol pre-seeded")
                    .push((file.clone(), lines));
            }
        }
    }
    for symbol in RETIRED_SYMBOLS {
        let hits = &hits_by_symbol[symbol];
        assert!(
            hits.is_empty(),
            "retired R22 symbol `{symbol}` reintroduced in production source. \
             The carrier-verdict + carrier-provenance substrate was deleted; \
             the typed-IR \
             `TypeExpr::SyntheticSlotBinding` variant replaces it at the \
             projector / registry / reducer surface. If the symbol is \
             legitimately back in use as a new construct, justify and \
             delete its entry from RETIRED_SYMBOLS.\nHits:\n{hits:#?}"
        );
    }
}

/// Self-test: synthetic-fixture proves the scanner catches a
/// retired symbol. A guard that does not discriminate is a stub.
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
// carrier roots on the content-free `SyntheticBindingId` cache identity —
// a `SemanticNodeId(<ident>.value_node)` cache-key construction is a
// FORBIDDEN ordinal bypass.
//
// Contract (per `[[component-meta-shallow-by-default-rule]]` and the
// `TypeExpr::SyntheticSlotBinding` rustdoc):
//   The synthetic carrier variant is a shallow terminal — projectors,
//   reducers, and the registry all refuse to resolve its `binding_name`
//   through `TypeRegistry`. The ONLY legitimate way to deepen a carrier
//   into its underlying member shape is to construct a
//   `ShapeCacheKey::synthetic_binding_whole(
//        SyntheticBindingId::from_carrier_key(&carrier), mode)`
//   key and consult `ShapeCacheDb`. The cache identity is the
//   content-free `SyntheticBindingId`
//   (`scope_canonical_id, surface_kind, slot_name, binding_name`); the
//   carrier's `value_node` arena ordinal is value-side PROVENANCE only —
//   it round-trips through `SemanticNodeData::SyntheticBinding` at the
//   compat materialisation boundary and NEVER enters the cache key.
//
//   A consumer that drills down by reading `SyntheticCarrierKey::\
//   value_node` and wrapping it in a fresh `SemanticNodeId(...)` to key
//   the cache is committing an ordinal bypass. That is the defect class
//   this guard prevents: the `value_node` ordinal is a store/generation-
//   relative arena index, NOT a content-free identity, so keying on it
//   (a) splits the cache per provenance ordinal (same-identity carriers
//   miss each other), (b) re-introduces the R6 violation the
//   content-free `SyntheticBindingId` was built to remove. There is no
//   longer any legitimate `SemanticNodeId(carrier.value_node)`
//   cache-key construction — the previous "cache-route" exemption is
//   removed.
//
//   STRUCTURAL CONFINEMENT IS PRIMARY. The first line of defense is the
//   sealed `NonSyntheticTypeExpr` newtype + the module-private
//   `ShapeSubject` / `ShapeCacheKey.subject` construction in
//   `component_meta_caches.rs`: a `SyntheticSlotBinding`-carrying
//   `TypeExpr` cannot become a `ShapeSubject::TypeExpr` structural-hash
//   subject (its `value_node` can never fold into the hash), a bare
//   carrier redirects to the content-free `ShapeSubject::SyntheticBinding`,
//   and a nested carrier is uncached. THIS scanner is the bounded
//   residual SYNTACTIC supplement to that structural mechanism — it
//   catches a hand-rolled `SemanticNodeId(_.value_node)` ordinal-key
//   construction that never goes through the sealed constructors at all.
//
//   The regular `ShapeSubject::MemberValueNode` route
//   (`member_shape_peek_or_compute`) takes the member BY REFERENCE
//   (`member: &SurfaceMember`) and reads `member.value` through the
//   sealed `MemberShapeNodeSubject` newtype — it NEVER writes
//   `SemanticNodeId(x.value_node)`, so it is structurally unmatched by
//   the scanner and unaffected.
//
//   Currently zero workspace consumers exercise the explicit-deepen
//   route — the carrier is always shallow in every projector,
//   reducer, registry, and graph-builder site. The
//   `tests/cases/g_misc0/synthetic_carrier_explicit_deepen_proof.rs` integration
//   test exercises the content-free cache-key identity via the
//   `ShapeCacheDb::insert_synthetic_carrier_deep_for_test` /
//   `get_synthetic_carrier_deep_for_test` `#[cfg(any(test,
//   debug_assertions))]` helpers, proving the route is well-defined
//   for any future consumer that needs it.

/// Identifier-boundary token-stream check: does `line` contain the
/// regex `SemanticNodeId\s*\(\s*([A-Za-z_][A-Za-z0-9_]*)\s*\.\s*value_node`?
///
/// This is the canonical shape of "construct a `SemanticNodeId` from
/// the `value_node` field of some struct value" — a forbidden ordinal
/// cache-key construction. It catches the suspicious pattern without
/// depending on the binding's name. False positives are possible only if
/// some future unrelated type also names a field `value_node` — in that
/// case the false-positive site must be explicitly allowlisted below.
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

/// Structural-confinement-vs-scanner record. The PRIMARY confinement for
/// the synthetic-deepen identity is structural: the sealed
/// `NonSyntheticTypeExpr` newtype + module-private `ShapeSubject` /
/// `ShapeCacheKey.subject` construction in `component_meta_caches.rs` keep
/// a `value_node`-bearing carrier out of every cache key by construction.
/// This grep scanner is the BOUNDED residual SYNTACTIC supplement: it
/// catches a hand-rolled `SemanticNodeId(_.value_node)` ordinal-key
/// construction that bypasses the sealed constructors entirely. The
/// previous "legitimate cache-route" exemption (an upstream
/// member-shape-key constructor window) is REMOVED — there is no
/// legitimate `SemanticNodeId(carrier.value_node)` cache-key construction
/// anymore, because the content-free `SyntheticBindingId` is the identity.
const SYNTHETIC_DEEPEN_CONFINEMENT: ConfinementRecord = ConfinementRecord {
    primary_mechanism: "structural",
    scanner_role: "residual-syntactic-supplement",
};

/// A small record documenting that the structural confinement is primary
/// and this scanner is the residual supplement (Structural-Confinement-
/// First). Carried as a const so a future edit that demotes the
/// structural mechanism has to consciously rewrite this record.
struct ConfinementRecord {
    primary_mechanism: &'static str,
    scanner_role: &'static str,
}

/// The architecture guard test: no production source file may construct a
/// `SemanticNodeId` from any struct's `value_node` field — the scanner's
/// syntactic predicate bans EVERY such construction outright (it does not
/// try to prove cache-key-ness from text). The architectural target it
/// enforces: the synthetic-deepen identity is the content-free
/// `SyntheticBindingId` via `ShapeCacheKey::synthetic_binding_whole`, and
/// the `value_node` arena ordinal is value-side provenance only, so a
/// `SemanticNodeId(_.value_node)` construction would be an ordinal
/// cache-key bypass. There is no exemption.
#[test]
fn synthetic_carrier_explicit_deepen_routes_through_shape_cache_key() {
    // Structural-Confinement-First: this scanner is the bounded residual
    // supplement to the sealed `NonSyntheticTypeExpr` + private
    // `ShapeSubject` construction, which is the PRIMARY mechanism.
    assert_eq!(
        SYNTHETIC_DEEPEN_CONFINEMENT.primary_mechanism, "structural",
        "the synthetic-deepen confinement's PRIMARY mechanism must remain \
         structural (the sealed `NonSyntheticTypeExpr` + private `ShapeSubject` \
         construction); this scanner is only the residual supplement"
    );
    assert_eq!(
        SYNTHETIC_DEEPEN_CONFINEMENT.scanner_role, "residual-syntactic-supplement",
        "this scanner must remain the residual syntactic supplement, never the \
         primary confinement"
    );

    let files = collect_production_sources();

    let mut violations: Vec<(PathBuf, Vec<usize>)> = Vec::new();
    for file in &files {
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        let processed = preprocess(&text);
        let violation_lines: Vec<usize> = processed
            .lines()
            .enumerate()
            .filter_map(|(i, l)| {
                if line_constructs_semantic_node_id_from_value_node(l) {
                    Some(i + 1)
                } else {
                    None
                }
            })
            .collect();
        if !violation_lines.is_empty() {
            violations.push((file.clone(), violation_lines));
        }
    }

    assert!(
        violations.is_empty(),
        "Architecture rule violation: production source constructs a \
         `SemanticNodeId` directly from a struct's `value_node` field. The \
         synthetic-carrier explicit-deepen route MUST root on the content-free \
         `SyntheticBindingId` via \
         `ShapeCacheKey::synthetic_binding_whole(\
         SyntheticBindingId::from_carrier_key(&carrier), mode)`. The \
         `value_node` arena ordinal is value-side provenance (re-attached at \
         the compat boundary via `to_carrier_key`/`raise`), NEVER a cache key. \
         A `SemanticNodeId(_.value_node)` construction is a forbidden ordinal \
         bypass: it splits the cache per provenance ordinal and re-introduces \
         the R6 violation the content-free identity removed. The regular \
         `ShapeSubject::MemberValueNode` route takes the member by reference \
         and reads `member.value` through the sealed `MemberShapeNodeSubject` \
         newtype, so it is unaffected. See \
         `[[component-meta-shallow-by-default-rule]]` in \
         `.claude/skills/component-meta/SKILL.md`.\n\nViolations:\n{violations:#?}"
    );
}

/// Self-test for the explicit-deepen guard: the per-line scanner catches
/// every `SemanticNodeId(<ident>.value_node)` ordinal-key construction —
/// including the shapes that were previously exempt as "legitimate
/// cache-route" split calls (the exemption is removed) — while the
/// genuinely-legit shapes (`SemanticNodeId(0)`,
/// `SemanticNodeId(member.id_field)`, `value_node:` field decls,
/// serialization) stay non-matching.
#[test]
fn synthetic_carrier_explicit_deepen_guard_self_test() {
    // -----------------------------------------------------------------
    // Forbidden shapes — every `SemanticNodeId(_.value_node)`
    // construction matches. There is no exemption.
    // -----------------------------------------------------------------

    let violation_a = "let id = SemanticNodeId(key.value_node);";
    let violation_b = "    SemanticNodeId(carrier.value_node)";
    let violation_c = "SemanticNodeId ( binding . value_node ) // whitespace tolerance";

    assert!(
        line_constructs_semantic_node_id_from_value_node(violation_a),
        "self-test: scanner missed `SemanticNodeId(key.value_node);` — \
         the canonical shape of a forbidden ordinal-key construction"
    );
    assert!(
        line_constructs_semantic_node_id_from_value_node(violation_b),
        "self-test: scanner missed `SemanticNodeId(carrier.value_node)`"
    );
    assert!(
        line_constructs_semantic_node_id_from_value_node(violation_c),
        "self-test: scanner missed whitespace-tolerant `SemanticNodeId ( binding . value_node )`"
    );

    // The PREVIOUSLY-exempt cache-route split shapes are now ALSO
    // violations — the `value_node` ordinal is provenance, never a cache
    // key, so even a member-shape-key constructor call wrapping a
    // `SemanticNodeId(carrier.value_node)` argument is a forbidden bypass.
    // The deref line is flagged regardless of any upstream cache-route
    // call.
    let formerly_exempt_split_call =
        "    crate::semantic_query::SemanticNodeId(carrier.value_node),";
    assert!(
        line_constructs_semantic_node_id_from_value_node(formerly_exempt_split_call),
        "self-test: scanner must now flag the formerly-exempt split-call deref — \
         the `SemanticNodeId(carrier.value_node)` cache-route exemption is removed"
    );

    // -----------------------------------------------------------------
    // Genuinely-legit shapes — must NOT trip the scanner.
    // -----------------------------------------------------------------

    // The content-free route does NOT construct a `SemanticNodeId` at all.
    let legit_content_free_route =
        "let key = ShapeCacheKey::synthetic_binding_whole(SyntheticBindingId::from_carrier_key(&carrier), mode);";
    let legit_field_decl = "pub value_node: u64,";
    let legit_struct_constr =
        "SyntheticCarrierKey { scope_canonical_id: s, surface_kind: k, slot_name: n, binding_name: b, value_node: 7 }";
    let legit_serialize = "key.value_node.to_string()";
    let legit_semantic_node_id_from_const = "let id = SemanticNodeId(0);";
    let legit_semantic_node_id_from_field_no_value_node =
        "let id = SemanticNodeId(member.id_field);";
    // The regular `MemberValueNode` route takes the member BY REFERENCE
    // and reads `member.value` through the sealed `MemberShapeNodeSubject`
    // newtype (not `SemanticNodeId(_.value_node)`) — unaffected.
    let legit_regular_member_route =
        "let key = ShapeCacheKey::surface_member_value_whole_with_context(scope, member, mode);";

    for legit in [
        legit_content_free_route,
        legit_field_decl,
        legit_struct_constr,
        legit_serialize,
        legit_semantic_node_id_from_const,
        legit_semantic_node_id_from_field_no_value_node,
        legit_regular_member_route,
    ] {
        assert!(
            !line_constructs_semantic_node_id_from_value_node(legit),
            "self-test: scanner false-positives on a legitimate line: `{legit}`"
        );
    }
}
