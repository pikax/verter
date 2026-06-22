//! Architecture guard: explicit deepening of a `SyntheticSlotBinding`
//! carrier roots on the content-free `SyntheticBindingId` cache identity —
//! a `SemanticNodeId(<ident>.value_node)` cache-key construction is a
//! FORBIDDEN ordinal bypass.
//!
//! Contract (per `[[component-meta-shallow-by-default-rule]]` and the
//! `TypeExpr::SyntheticSlotBinding` rustdoc):
//!   The synthetic carrier variant is a shallow terminal — projectors,
//!   reducers, and the registry all refuse to resolve its `binding_name`
//!   through `TypeRegistry`. The ONLY legitimate way to deepen a carrier
//!   into its underlying member shape is to construct a
//!   `ShapeCacheKey::synthetic_binding_whole(
//!        SyntheticBindingId::from_carrier_key(&carrier), mode)`
//!   key and consult `ShapeCacheDb`. The cache identity is the
//!   content-free `SyntheticBindingId`
//!   (`scope_canonical_id, surface_kind, slot_name, binding_name`); the
//!   carrier's `value_node` arena ordinal is value-side PROVENANCE only —
//!   it round-trips through `SemanticNodeData::SyntheticBinding` at the
//!   compat materialisation boundary and NEVER enters the cache key.
//!
//!   A consumer that drills down by reading `SyntheticCarrierKey::\
//!   value_node` and wrapping it in a fresh `SemanticNodeId(...)` to key
//!   the cache is committing an ordinal bypass. That is the defect class
//!   this guard prevents: the `value_node` ordinal is a store/generation-
//!   relative arena index, NOT a content-free identity, so keying on it
//!   (a) splits the cache per provenance ordinal (same-identity carriers
//!   miss each other), (b) re-introduces the R6 violation the
//!   content-free `SyntheticBindingId` was built to remove. There is no
//!   longer any legitimate `SemanticNodeId(carrier.value_node)`
//!   cache-key construction — the previous "cache-route" exemption is
//!   removed.
//!
//!   STRUCTURAL CONFINEMENT IS PRIMARY. The first line of defense is the
//!   sealed `NonSyntheticTypeExpr` newtype + the module-private
//!   `ShapeSubject` / `ShapeCacheKey.subject` construction in
//!   `component_meta_caches.rs`: a `SyntheticSlotBinding`-carrying
//!   `TypeExpr` cannot become a `ShapeSubject::TypeExpr` structural-hash
//!   subject (its `value_node` can never fold into the hash), a bare
//!   carrier redirects to the content-free `ShapeSubject::SyntheticBinding`,
//!   and a nested carrier is uncached. THIS scanner is the bounded
//!   residual SYNTACTIC supplement to that structural mechanism — it
//!   catches a hand-rolled `SemanticNodeId(_.value_node)` ordinal-key
//!   construction that never goes through the sealed constructors at all.
//!
//!   The regular `ShapeSubject::MemberValueNode` route
//!   (`member_shape_peek_or_compute`) takes the member BY REFERENCE
//!   (`member: &SurfaceMember`) and reads `member.value` through the
//!   sealed `MemberShapeNodeSubject` newtype — it NEVER writes
//!   `SemanticNodeId(x.value_node)`, so it is structurally unmatched by
//!   the scanner and unaffected.
//!
//!   Currently zero workspace consumers exercise the explicit-deepen
//!   route — the carrier is always shallow in every projector,
//!   reducer, registry, and graph-builder site. The
//!   `tests/cases/g_misc0/synthetic_carrier_explicit_deepen_proof.rs` integration
//!   test exercises the content-free cache-key identity via the
//!   `ShapeCacheDb::insert_synthetic_carrier_deep_for_test` /
//!   `get_synthetic_carrier_deep_for_test` `#[cfg(any(test,
//!   debug_assertions))]` helpers, proving the route is well-defined
//!   for any future consumer that needs it.
//!
//! GUARD-LOCAL SCANNER RECORD (this is a bounded residual SYNTACTIC
//! supplement, NOT a structural assert):
//!
//!   scanner_invariant=synthetic_deepen_no_hand_rolled_semantic_node_id_from_value_node
//!   scanner_justification=Structural confinement (sealed NonSyntheticTypeExpr
//!     + module-private ShapeSubject/ShapeCacheKey construction + sealed
//!     MemberShapeNodeSubject) is PRIMARY, but cannot see a hand-rolled
//!     `SemanticNodeId(<ident>.value_node)` written outside the sealed
//!     constructors — `SemanticNodeId` is a public tuple struct and
//!     `SyntheticCarrierKey.value_node` is a public u64, so a raw
//!     ordinal-key construction is still writable. This scanner is the
//!     bounded residual SYNTACTIC supplement to that structural primary.
//!   mechanism_ruling=binding neutral architecture design ruling for this
//!     work unit: this scanner is NOT structurally impossible today
//!     (SemanticNodeId is a public tuple struct; SyntheticCarrierKey.value_node
//!     is a public u64), so it survives as a permitted bounded residual
//!     supplement and MUST carry this full durable record.
//!   hardening_rounds=1
//!   hardening_history=
//!     - first hardening pass: broadened the match from a per-LINE scan
//!       to a whitespace/newline-tolerant STREAM scan over the whole
//!       preprocessed source (byte offsets mapped back to line numbers
//!       for the report). WHY: the line-based form admitted a multi-line
//!       false negative — a split construction
//!       `SemanticNodeId(\n    carrier.value_node\n)` straddles a newline
//!       and was therefore invisible to a per-line matcher, contradicting
//!       the scanner's whitespace-tolerant / "ANY construction" claim.
//!       The SPELLING set is UNCHANGED — the needle is still
//!       `SemanticNodeId(<ident>.value_node)`; only line-vs-stream
//!       matching changed (within the two-pass hardening bound).

use std::path::{Path, PathBuf};

/// File names whose presence at the head of the path should make us
/// self-exclude: the sibling `architecture_guards.rs`, which carries
/// literal `value_node` needle strings for its own structural assertions.
/// (This guard file is auto-excluded by `is_test_file` — it lives under a
/// `tests/` segment — so it never needs an explicit name entry.)
const SELF_EXCLUDED_FILE_NAMES: &[&str] = &["architecture_guards.rs"];

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
///
/// A directory we are asked to walk MUST be readable — a `read_dir`
/// failure is a hard error, never a silent `return` that would drop a
/// whole subtree from the scan and let the guard pass vacuously. Each
/// `DirEntry` is unwrapped with a panic carrying the directory, and the
/// dir-vs-file classification uses `entry.file_type()` (panic-on-error),
/// never `path.is_dir()`/`path.is_file()` — those collapse a metadata
/// error to `false` and would silently drop a file or whole subtree (the
/// per-entry fail-open class this hard-failing traversal closes).
fn collect_production_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = std::fs::read_dir(dir).unwrap_or_else(|e| {
        panic!(
            "scan integrity: failed to read directory `{}`: {e}",
            dir.display()
        )
    });
    for entry in entries {
        let entry = entry.unwrap_or_else(|e| {
            panic!(
                "scan integrity: failed to read a directory entry under `{}`: {e}",
                dir.display()
            )
        });
        let path = entry.path();
        let file_type = entry.file_type().unwrap_or_else(|e| {
            panic!(
                "scan integrity: failed to read the file type of `{}`: {e}",
                path.display()
            )
        });
        if file_type.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name == "target" || name == "node_modules" || name == ".git" {
                continue;
            }
            collect_production_rs(&path, out);
        } else if file_type.is_file()
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

/// Hard-failing directory classification for a path we discovered by
/// name (a `crates/*` child, or its `src/` subdirectory). Returns
/// whether the path is a directory.
///
/// A genuinely-absent path (`ErrorKind::NotFound`) is a legitimate
/// non-directory answer (`false`) — a crate root without a `src/` is
/// simply skipped. ANY OTHER metadata IO error (permissions, a
/// `NotADirectory` traversal, a stale handle) is a hard panic carrying
/// the path: `Path::is_dir()` collapses every such error to `false` and
/// would silently drop a crate or whole `src/` subtree from the scan,
/// the exact fail-open class this guard must close. Mirrors the
/// per-entry hard-failing discipline of `collect_production_rs`.
fn classified_as_dir(path: &Path) -> bool {
    match std::fs::metadata(path) {
        Ok(meta) => meta.is_dir(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
        Err(e) => panic!(
            "scan integrity: failed to classify `{}`: {e}",
            path.display()
        ),
    }
}

/// Collect every production `.rs` file under `crates/*/src/`.
///
/// The `crates/` directory MUST be readable — a `read_dir` failure is a
/// hard error, never a silent empty return that would let the guard pass
/// vacuously. The per-crate dir-vs-file classification uses
/// `entry.file_type()` (panic-on-error) and the `src/` subdirectory uses
/// hard-failing metadata (`classified_as_dir`); neither collapses a
/// metadata IO error to a silent skip.
fn collect_production_sources() -> Vec<PathBuf> {
    let root = workspace_root();
    let mut files: Vec<PathBuf> = Vec::new();
    let crates_dir = root.join("crates");
    let crates = std::fs::read_dir(&crates_dir).unwrap_or_else(|e| {
        panic!(
            "scan integrity: failed to read crates directory `{}`: {e}",
            crates_dir.display()
        )
    });
    for entry in crates {
        // Unwrap each `DirEntry` with a panic carrying the directory — no
        // `.flatten()` that would silently drop a crate dir whose entry
        // errored.
        let entry = entry.unwrap_or_else(|e| {
            panic!(
                "scan integrity: failed to read a crates directory entry under `{}`: {e}",
                crates_dir.display()
            )
        });
        let crate_dir = entry.path();
        // Classify the crate-root entry by its `file_type()` (panic-on-
        // error), never `path.is_dir()` which would collapse a metadata
        // error to a silent skip.
        let crate_dir_is_dir = entry
            .file_type()
            .unwrap_or_else(|e| {
                panic!(
                    "scan integrity: failed to read the file type of `{}`: {e}",
                    crate_dir.display()
                )
            })
            .is_dir();
        if !crate_dir_is_dir {
            continue;
        }
        let src = crate_dir.join("src");
        if !classified_as_dir(&src) {
            continue;
        }
        collect_production_rs(&src, &mut files);
    }
    files
}

/// Identifier-boundary token-STREAM scan over a whole (preprocessed)
/// source: returns the byte offset of every
/// `SemanticNodeId\s*\(\s*([A-Za-z_][A-Za-z0-9_]*)\s*\.\s*value_node`
/// construction.
///
/// This is the canonical shape of "construct a `SemanticNodeId` from the
/// `value_node` field of some struct value" — a forbidden ordinal
/// cache-key construction. It catches the suspicious pattern without
/// depending on the binding's name.
///
/// Whole-source STREAM scan, not per-line: the matcher's inter-token
/// whitespace skips include NEWLINES, so a construction that straddles
/// line breaks — `SemanticNodeId(\n    carrier.value_node\n)` — is
/// caught. A per-line matcher cannot see a multi-line split; the
/// stream scan can. The SPELLING set is the same single needle either
/// way: `SemanticNodeId(<ident>.value_node)`.
///
/// False positives are possible only if some future unrelated type also
/// names a field `value_node` — in that case the false-positive site must
/// be explicitly allowlisted.
fn value_node_construction_offsets(src: &str) -> Vec<usize> {
    let bytes = src.as_bytes();
    let needle = b"SemanticNodeId";
    let is_ident_char = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let mut hits = Vec::new();
    let mut i = 0usize;
    while i + needle.len() <= bytes.len() {
        if &bytes[i..i + needle.len()] != needle {
            i += 1;
            continue;
        }
        let match_start = i;
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
        // Skip whitespace (INCLUDING newlines — this is the whole-source
        // stream-scan property a per-line matcher lacks).
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
        hits.push(match_start);
        // Advance past this match start to find further constructions.
        i = match_start + needle.len();
    }
    hits
}

/// Map a byte offset in `src` to its 1-based line number (for the
/// violation report). Counts newlines up to (not including) `offset`.
fn byte_offset_to_line(src: &str, offset: usize) -> usize {
    let upto = offset.min(src.len());
    src.as_bytes()[..upto]
        .iter()
        .filter(|&&b| b == b'\n')
        .count()
        + 1
}

/// True iff the (single- or multi-line) `text` contains ANY
/// `SemanticNodeId(<ident>.value_node)` construction. Stream-scan based
/// (newline-tolerant), so a split construction is caught.
fn constructs_semantic_node_id_from_value_node(text: &str) -> bool {
    !value_node_construction_offsets(text).is_empty()
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

    // Prove the scan is not vacuous — a sentinel production file that
    // MUST exist (the shape-cache key/subject owner) has to be in the
    // collected set. If the traversal returned empty/partial, this fails
    // loudly instead of passing silently.
    let sentinel = std::path::Path::new("crates")
        .join("verter_session")
        .join("src")
        .join("component_meta_caches.rs");
    assert!(
        files.iter().any(|f| f.ends_with(&sentinel)),
        "scan vacuity guard: the sentinel production file `{}` was NOT in the collected \
         source set ({} files). A guard that walks production source must prove it \
         actually scanned the expected files; an empty/partial traversal must not pass \
         vacuously.",
        sentinel.display(),
        files.len(),
    );

    let mut violations: Vec<(PathBuf, Vec<usize>)> = Vec::new();
    for file in &files {
        // A read failure on a file the walk already classified as
        // production source is a hard error — never a silent skip that
        // could drop a violating file from the scan.
        let text = std::fs::read_to_string(file).unwrap_or_else(|e| {
            panic!(
                "scan integrity: failed to read production source `{}`: {e}",
                file.display()
            )
        });
        let processed = preprocess(&text);
        // STREAM scan over the WHOLE preprocessed source (not per-line),
        // then map each match's byte offset back to a 1-based line number
        // for the report — so a multi-line split construction is caught.
        let violation_lines: Vec<usize> = value_node_construction_offsets(&processed)
            .into_iter()
            .map(|off| byte_offset_to_line(&processed, off))
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

/// Self-test for the explicit-deepen guard: the STREAM scanner catches
/// every `SemanticNodeId(<ident>.value_node)` ordinal-key construction —
/// including the shapes that were once exempt as "legitimate
/// cache-route" split calls (no such exemption exists) AND a MULTI-LINE
/// split construction (a multi-line shape no per-line matcher can see) —
/// while the genuinely-legit shapes (`SemanticNodeId(0)`,
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
        constructs_semantic_node_id_from_value_node(violation_a),
        "self-test: scanner missed `SemanticNodeId(key.value_node);` — \
         the canonical shape of a forbidden ordinal-key construction"
    );
    assert!(
        constructs_semantic_node_id_from_value_node(violation_b),
        "self-test: scanner missed `SemanticNodeId(carrier.value_node)`"
    );
    assert!(
        constructs_semantic_node_id_from_value_node(violation_c),
        "self-test: scanner missed whitespace-tolerant `SemanticNodeId ( binding . value_node )`"
    );

    // The MULTI-LINE split construction. No single line contains the whole
    // `SemanticNodeId(...value_node)` shape, so a per-line matcher cannot
    // see it. The whole-source stream scan catches it, and reports the line
    // of the `SemanticNodeId` token.
    let multiline_split = "let id = SemanticNodeId(\n    carrier.value_node\n);";
    assert!(
        constructs_semantic_node_id_from_value_node(multiline_split),
        "self-test: the STREAM scanner must catch a multi-line split \
         `SemanticNodeId(\\n    carrier.value_node\\n)` construction — no per-line \
         matcher can see this shape"
    );
    let offsets = value_node_construction_offsets(multiline_split);
    assert_eq!(
        offsets.len(),
        1,
        "self-test: the multi-line split must register as exactly ONE \
         construction; got {offsets:?}"
    );
    assert_eq!(
        byte_offset_to_line(multiline_split, offsets[0]),
        1,
        "self-test: the violation must be reported on the line of the \
         `SemanticNodeId` token (line 1 here), via byte-offset → line-number mapping"
    );

    // A multi-line split with extra leading lines maps to the correct line.
    let multiline_offset_line =
        "fn f() {\n    let id = SemanticNodeId(\n        carrier.value_node,\n    );\n}";
    let off2 = value_node_construction_offsets(multiline_offset_line);
    assert_eq!(
        off2.len(),
        1,
        "self-test: one split construction; got {off2:?}"
    );
    assert_eq!(
        byte_offset_to_line(multiline_offset_line, off2[0]),
        2,
        "self-test: byte-offset → line mapping must place the construction on \
         line 2 (where `SemanticNodeId` appears)"
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
        constructs_semantic_node_id_from_value_node(formerly_exempt_split_call),
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
    // A multi-line construction reading a DIFFERENT field (`member.id`, no
    // `value_node`) must NOT trip — proving the stream scan still pins the
    // exact `value_node` field, not "any `SemanticNodeId(\n ident.field`".
    let legit_multiline_other_field = "let id = SemanticNodeId(\n    member.id_field\n);";

    for legit in [
        legit_content_free_route,
        legit_field_decl,
        legit_struct_constr,
        legit_serialize,
        legit_semantic_node_id_from_const,
        legit_semantic_node_id_from_field_no_value_node,
        legit_regular_member_route,
        legit_multiline_other_field,
    ] {
        assert!(
            !constructs_semantic_node_id_from_value_node(legit),
            "self-test: scanner false-positives on a legitimate snippet: `{legit}`"
        );
    }
}

/// Discriminating self-test for the top-level crate/`src` classifier:
/// `classified_as_dir` must hard-fail on a metadata IO error rather than
/// silently treating the path as a non-directory. This is the precise
/// difference from `Path::is_dir()`, which collapses EVERY metadata error
/// (including a `NotADirectory`/permission error) to `false` and would
/// drop a crate or whole `src/` subtree from the scan vacuously.
#[test]
fn synthetic_deepen_scanner_classified_as_dir_hard_fails_on_metadata_error_self_test() {
    let scratch = std::env::temp_dir().join(format!(
        "verter_synth_classify_{}_{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&scratch).expect("create scratch dir");

    // A real directory classifies as a directory.
    assert!(
        classified_as_dir(&scratch),
        "classifier must report an existing directory as a directory"
    );

    // A genuinely-absent path (`NotFound`) is a LEGITIMATE non-directory
    // answer — a crate root without a `src/` is simply skipped, NOT a
    // panic.
    let absent = scratch.join("definitely_absent").join("src");
    assert!(
        !classified_as_dir(&absent),
        "classifier must report a genuinely-absent (NotFound) path as a \
         non-directory WITHOUT panicking — a missing `src/` is a legitimate skip"
    );

    // A path that traverses THROUGH a regular file as if it were a
    // directory produces a non-`NotFound` metadata IO error
    // (`NotADirectory` on Unix, an analogous non-NotFound kind on
    // Windows). `classified_as_dir` MUST panic on it; `Path::is_dir()`
    // would silently return `false` (the fail-open class this guard
    // closes). We assert the panic discriminates the hard-failing
    // classifier from a collapsing `is_dir()` classification.
    let regular_file = scratch.join("regular.txt");
    std::fs::write(&regular_file, b"not a directory").expect("write regular file");
    let through_file = regular_file.join("src");

    // Sanity: confirm this scratch path is the IO-error (NOT NotFound)
    // case on this platform, so the test discriminates rather than
    // passing vacuously on a platform where the path resolves to
    // NotFound.
    let probe = std::fs::metadata(&through_file);
    assert!(
        probe
            .as_ref()
            .err()
            .is_some_and(|e| e.kind() != std::io::ErrorKind::NotFound),
        "self-test precondition: traversing through a regular file must \
         yield a non-NotFound metadata IO error on this platform; got {probe:?}"
    );

    let panicked = std::panic::catch_unwind(|| classified_as_dir(&through_file)).is_err();
    assert!(
        panicked,
        "classifier must HARD-FAIL (panic) on a non-NotFound metadata IO \
         error instead of silently treating the path as a non-directory. \
         `Path::is_dir()` would return `false` here, dropping a subtree \
         from the scan — that fail-open is exactly what this classifier closes."
    );

    std::fs::remove_dir_all(&scratch).ok();
}
