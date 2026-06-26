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
//!
//! GUARD-LOCAL SCANNER RECORD (this is a justified identity-absence
//! scanner, NOT a structural assert):
//!
//!   scanner_invariant=r22_carrier_verdict_substrate_absent_from_production
//!   scanner_justification=Rust cannot prove "this identifier is absent
//!     from the entire production codebase and must never be reintroduced"
//!     — a compile-fail import would catch only a PUBLIC reintroduction,
//!     not a private type/module/field/accessor; retired-symbol absence is
//!     expressible only as a name-spelling source scan.
//!   mechanism_ruling=binding architecture design ruling
//!     (see `docs/arch/cache-key-guard-mechanism-rulings.md`): a source scanner
//!     is the justified mechanism for R22 retired-symbol absence (Rust cannot
//!     prove "identifier absent from the whole codebase / never reintroduced").
//!   hardening_rounds=0
//!   hardening_history=none — record adopted at this reorganization; no
//!     spelling-case add/broaden.

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
///
/// A directory we are asked to walk MUST be readable — a `read_dir`
/// failure is a hard error, never a silent `return` that would drop a
/// whole subtree from the scan and let the gate pass vacuously. Each
/// `DirEntry` is unwrapped with a panic carrying the directory, and the
/// dir-vs-file classification uses `entry.file_type()` (panic-on-error),
/// never `path.is_dir()`/`path.is_file()` — those collapse a metadata
/// error to `false` and would silently drop a file or whole subtree from
/// the scan (the per-entry fail-open class this hard-failing traversal
/// closes).
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
/// hard error, never a silent empty return that would let the gate pass
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

/// Main gate: every retired R22 symbol must be ABSENT from
/// production source.
#[test]
fn no_carrier_verdict_db_in_production() {
    let files = collect_production_sources();

    // Prove the scan is not vacuous — a sentinel production file that MUST
    // exist has to be in the collected set. If the traversal returned
    // empty/partial, this fails loudly instead of passing silently.
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

    // Read + preprocess each file ONCE, then test every retired symbol against
    // the cached processed text (was O(symbols × files) of redundant reads).
    let mut hits_by_symbol: std::collections::BTreeMap<&str, Vec<(PathBuf, Vec<usize>)>> =
        RETIRED_SYMBOLS.iter().map(|s| (*s, Vec::new())).collect();
    for file in &files {
        // A read failure on a file the walk already classified as
        // production source is a hard error — never a silent skip that
        // could drop a file carrying a retired symbol from the scan.
        let text = std::fs::read_to_string(file).unwrap_or_else(|e| {
            panic!(
                "scan integrity: failed to read production source `{}`: {e}",
                file.display()
            )
        });
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

/// Discriminating self-test for the top-level crate/`src` classifier:
/// `classified_as_dir` must hard-fail on a metadata IO error rather than
/// silently treating the path as a non-directory. This is the precise
/// difference from `Path::is_dir()`, which collapses EVERY metadata error
/// (including a `NotADirectory`/permission error) to `false` and would
/// drop a crate or whole `src/` subtree from the scan vacuously.
#[test]
fn retired_symbol_scanner_classified_as_dir_hard_fails_on_metadata_error_self_test() {
    let scratch = std::env::temp_dir().join(format!(
        "verter_no_carrier_classify_{}_{:?}",
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
