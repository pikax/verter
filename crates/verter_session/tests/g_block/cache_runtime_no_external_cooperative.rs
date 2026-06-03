//! Architecture guard: the `cooperative_*` cold-compute admission
//! primitives are cache-runtime-internal. Only `crates/verter_session/
//! src/cache_runtime/**` may name them. After the C2 cutover every
//! query-identity and content-addressed cache routes its cold build
//! through a cache-runtime node entry point (`lookup` / `query::lookup`)
//! or the consumer-facing `*Db::get_or_compute_admit` wrapper, never the
//! raw primitive. A production reference outside `cache_runtime` is a
//! regression and must fail this static-grep gate.
//!
//! Discipline mirrors `no_carrier_verdict_db.rs`:
//!
//!  - scans ONLY `crates/verter_session/src/**/*.rs` (production source),
//!  - skips `_tests.rs` / `tests.rs` / files under a `tests/` segment,
//!  - skips everything under `src/cache_runtime/` (the primitives' home),
//!  - strips line, block, and `#[cfg(test)] mod` modules before matching
//!    (so doc comments and inline tests do not trip the gate),
//!  - matches each symbol at identifier boundaries.
//!
//! Self-exclusion: this guard file lives under `tests/`, so it is skipped
//! by the production-source walk.

use std::path::{Path, PathBuf};

/// The cache-runtime-internal primitives. Any occurrence in
/// `crates/verter_session/src/**` OUTSIDE `src/cache_runtime/` (and
/// outside test files) is a regression.
const FORBIDDEN_PRIMITIVES: &[&str] = &[
    "cooperative_get_or_insert",
    "cooperative_get_or_insert_with_post_publish",
    "cooperative_admit_with_post_publish",
    "cooperative_admit_with_post_publish_by_flight_key",
    "cooperative_admit_with_lookup_publish",
];

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

/// True for test-only files (`*_tests.rs` / `tests.rs`, or anything under
/// a `tests/` path segment).
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

/// True for any path inside `crates/verter_session/src/cache_runtime/`,
/// the primitives' own home (where naming them is legitimate).
fn is_cache_runtime(path: &Path) -> bool {
    let mut prev_was_cache_runtime_parent = false;
    for c in path.components() {
        let seg = c.as_os_str().to_str().unwrap_or_default();
        if prev_was_cache_runtime_parent && seg == "cache_runtime" {
            return true;
        }
        prev_was_cache_runtime_parent = seg == "src";
    }
    false
}

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
            && !is_cache_runtime(&path)
        {
            out.push(path);
        }
    }
}

/// Replace `//` line comments and `/* ... */` block comments with
/// whitespace, preserving newlines. Skips comment-like sequences inside
/// string literals.
fn strip_comments(src: &str) -> String {
    let bytes = src.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let n = bytes.len();
    let mut i = 0usize;
    while i < n {
        let c = bytes[i];
        // Raw string: r"..." / r#"..."# / ...
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
        }
        // Regular string literal with \" escapes.
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
        // Line comment.
        if c == b'/' && i + 1 < n && bytes[i + 1] == b'/' {
            let mut k = i;
            while k < n && bytes[k] != b'\n' {
                out.push(b' ');
                k += 1;
            }
            i = k;
            continue;
        }
        // Block comment (nesting-aware).
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

/// Whitespace-out the body of every `#[cfg(test)] mod NAME { ... }` block
/// (newlines preserved) so inline test modules are not classed as
/// production.
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

/// Identifier-boundary matcher: `ident` matches ONLY when bounded by
/// non-identifier characters, so `cooperative_admit_with_post_publish`
/// does NOT spuriously match inside
/// `cooperative_admit_with_post_publish_by_flight_key` (which is itself a
/// separate forbidden needle).
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

fn collect_verter_session_production_sources() -> Vec<PathBuf> {
    let root = workspace_root();
    let mut files: Vec<PathBuf> = Vec::new();
    let src = root.join("crates").join("verter_session").join("src");
    collect_production_rs(&src, &mut files);
    files
}

/// The gate: no `verter_session` production source OUTSIDE `cache_runtime`
/// may name a `cooperative_*` primitive.
#[test]
fn no_external_cooperative_primitive_callers() {
    let files = collect_verter_session_production_sources();
    assert!(
        !files.is_empty(),
        "the production-source walk found no files — the scanner is broken"
    );

    for symbol in FORBIDDEN_PRIMITIVES {
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
            "cache-runtime-internal primitive `{symbol}` is referenced in \
             `verter_session` production source OUTSIDE `src/cache_runtime/`. \
             After the C2 cutover every cache routes its cold build through a \
             cache-runtime node entry point (`lookup` / `query::lookup`) or a \
             `*Db::get_or_compute_admit` wrapper — never the raw primitive. \
             Route the cold build through the node surface instead.\nHits:\n{hits:#?}"
        );
    }
}

/// Self-test: the scanner catches a forbidden primitive in a synthetic
/// production-shape line, AND the identifier-boundary discipline does not
/// false-positive a strict superstring (so the `_by_flight_key` superset
/// needle does not make the bare needle match its prefix).
#[test]
fn no_external_cooperative_guard_self_test() {
    let violation = "    let v = cooperative_admit_with_post_publish(map, inflight, key);";
    let processed = preprocess(violation);
    assert!(
        processed
            .lines()
            .any(|l| line_contains_identifier(l, "cooperative_admit_with_post_publish")),
        "self-test: the scanner failed to catch a forbidden primitive in a \
         synthetic production line"
    );

    // The bare `cooperative_admit_with_post_publish` needle must NOT match
    // inside the longer `cooperative_admit_with_post_publish_by_flight_key`
    // identifier (identifier-boundary discipline) — the longer name is a
    // separate needle that matches on its own.
    let superstring = "cooperative_admit_with_post_publish_by_flight_key(map, inflight, mk, fk);";
    assert!(
        !line_contains_identifier(superstring, "cooperative_admit_with_post_publish"),
        "self-test: the bare needle false-matched its superstring identifier"
    );
    assert!(
        line_contains_identifier(
            superstring,
            "cooperative_admit_with_post_publish_by_flight_key"
        ),
        "self-test: the superstring needle failed to match its own identifier"
    );

    // A comment mention must be stripped (no false positive).
    let comment = "// cooperative_get_or_insert is the bare admission shape";
    let processed_comment = preprocess(comment);
    assert!(
        !processed_comment
            .lines()
            .any(|l| line_contains_identifier(l, "cooperative_get_or_insert")),
        "self-test: a comment mention of a primitive must be stripped"
    );
}
