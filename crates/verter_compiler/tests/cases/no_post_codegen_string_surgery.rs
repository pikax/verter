//! `CodeTransform` single-source-of-truth architecture guard.
//!
//! All modifications to generated code MUST go through `CodeTransform`
//! operations (`overwrite`, `prepend_left`, `append_left`, the format-sink
//! emission APIs on `CodeGenOutput`, …) before `build_string()` produces the
//! final output. Mutating the *built* output string after the fact —
//! `String::replace`, `replace_range`, `replacen`, `insert_str`, a regex
//! substitution, or a manual slice-and-rejoin splice — desyncs the byte
//! offsets that the source map was built from, landing hovers and
//! go-to-definition on the wrong tokens.
//!
//! This guard source-scans every production `.rs` file under
//! `crates/verter_compiler/src` and FAILS if any of them rewrites a
//! `build_string()` result via string surgery. It catches four shapes:
//!
//! 1. **Direct chain** — `…build_string().replace(…)` (surgery chained
//!    straight onto the build call).
//! 2. **Bound-then-mutated** — `let s = …build_string();` followed by
//!    `s.replace(…)` (or another receiver-form surgery method) later in the
//!    same file.
//! 3. **Regex / argument replacement** — `let s = …build_string();` then a
//!    `replace`-family call taking it as the haystack, e.g.
//!    `re.replace_all(&s, …)`.
//! 4. **Manual splice/rebuild** — `let s = …build_string();` then `s` sliced
//!    and reassembled into a new string, e.g.
//!    `format!("{}{}", &s[..a], &s[b..])` or `s[..a].to_string() + &s[b..]`.
//!    The rejoin is matched per LOGICAL statement, so a `format!`/concat splice
//!    that rustfmt wraps across several lines (one slice per line) is caught
//!    just like its single-line form.
//!
//! Test surfaces (`*_tests.rs`, `tests.rs`, and anything under a `tests/`
//! directory) are excluded: the rule governs production code, and test
//! helpers legitimately normalise built output (e.g. CRLF folding) before
//! comparison.
//!
//! It is discriminating: [`detects_direct_chain_surgery`],
//! [`detects_bound_then_mutated_surgery`], [`detects_regex_replace_of_built_output`],
//! [`detects_manual_splice_of_built_output`],
//! [`detects_multiline_format_splice_of_built_output`], and
//! [`detects_multiline_concat_splice_of_built_output`] prove the detector fires
//! on each anti-pattern (single-line and rustfmt-wrapped multi-line), while
//! [`accepts_clean_codegen_usage`], [`accepts_readonly_slice_of_built_output`],
//! [`accepts_multiline_format_with_single_built_slice`],
//! [`accepts_separate_single_slice_statements`], and
//! [`ignores_surgery_in_comment_examples`] prove it does not fire on legitimate
//! `build_string()` consumption (including a single read-only slice, multi-line
//! or not) or commented examples.

use std::path::{Path, PathBuf};

/// Receiver-form string-surgery methods that, when called ON a `build_string()`
/// result, rewrite generated content out from under the source map.
const SURGERY_METHODS: &[&str] = &["replace", "replacen", "replace_range", "insert_str"];

/// Replace-family method names that rewrite the built output when it is passed
/// as the HAYSTACK argument — e.g. `regex.replace_all(&built, …)`. `replace_all`
/// is regex-only; `replace`/`replacen` cover both `str` and `Regex` receivers.
const HAYSTACK_REPLACE_METHODS: &[&str] = &["replace", "replacen", "replace_all"];

/// Root of the `verter_compiler` crate (`crates/verter_compiler`).
fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Whether `path` is a test surface that the production-code rule does not
/// govern: a `*_tests.rs` / `tests.rs` file, or anything nested under a
/// `tests/` directory segment.
fn is_test_surface(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    if name == "tests.rs" || name.ends_with("_tests.rs") {
        return true;
    }
    path.components().any(|c| c.as_os_str() == "tests")
}

/// Recursively collect production `.rs` files under `dir`.
fn collect_production_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(it) => it,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            collect_production_rs_files(&p, out);
        } else if p.extension().and_then(|e| e.to_str()) == Some("rs") && !is_test_surface(&p) {
            out.push(p);
        }
    }
}

/// Reconstruct source with comment-only lines dropped (so a "don't do this"
/// example inside a doc comment never trips the scanner), preserving line
/// structure for newline-spanning chains.
fn strip_comment_lines(src: &str) -> String {
    src.lines()
        .map(|line| {
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") {
                "" // keep the line slot (newline) but drop comment content
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// True if, after a `build_string()` occurrence at byte index `end_of_call`,
/// the next non-whitespace run begins with `.<surgery>(`.
fn chain_after_is_surgery(code: &str, end_of_call: usize) -> bool {
    let rest = code[end_of_call..].trim_start();
    let Some(after_dot) = rest.strip_prefix('.') else {
        return false;
    };
    let after_dot = after_dot.trim_start();
    SURGERY_METHODS.iter().any(|m| {
        after_dot
            .strip_prefix(m)
            .map(|tail| tail.trim_start().starts_with('('))
            .unwrap_or(false)
    })
}

/// Extract the identifier bound by a `let [mut] <ident>` line, if any.
fn binding_ident(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    let rest = trimmed.strip_prefix("let ")?;
    let rest = rest.strip_prefix("mut ").unwrap_or(rest);
    let end = rest.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))?;
    if end == 0 {
        return None;
    }
    Some(&rest[..end])
}

/// True if `line` calls a replace-family method (`replace` / `replacen` /
/// `replace_all`) with the built binding `ident` (optionally `&`-borrowed) as
/// the FIRST argument — i.e. the built output is the haystack being rewritten,
/// as in `re.replace_all(&built, "")`. Matching the first argument (not the
/// receiver) is what separates this from [`SURGERY_METHODS`]: there the binding
/// is the receiver; here it is the value handed to a regex/str substitution.
fn replace_takes_built_haystack(line: &str, ident: &str) -> bool {
    for method in HAYSTACK_REPLACE_METHODS {
        let call = format!("{method}(");
        let mut from = 0;
        while let Some(rel) = line[from..].find(&call) {
            let after = from + rel + call.len();
            let arg = line[after..].trim_start();
            let arg = arg.strip_prefix('&').map(str::trim_start).unwrap_or(arg);
            if let Some(tail) = arg.strip_prefix(ident) {
                // The first argument must be exactly the built binding,
                // delimited by a non-identifier char (`,`, `.`, `)`, ws).
                let boundary = tail
                    .chars()
                    .next()
                    .map(|c| !(c.is_ascii_alphanumeric() || c == '_'))
                    .unwrap_or(true);
                if boundary {
                    return true;
                }
            }
            from = after;
        }
    }
    false
}

/// Count word-boundary-delimited slice/index expressions on the built binding
/// `ident` in `line` — occurrences of `ident[` where `ident` is not the tail of
/// a longer identifier (so `prebuilt[` does not count as a slice of `built`).
fn count_built_slices(line: &str, ident: &str) -> usize {
    let needle = format!("{ident}[");
    let mut count = 0;
    let mut from = 0;
    while let Some(rel) = line[from..].find(&needle) {
        let abs = from + rel;
        let prev_is_ident = line[..abs]
            .chars()
            .next_back()
            .map(|c| c.is_ascii_alphanumeric() || c == '_')
            .unwrap_or(false);
        if !prev_is_ident {
            count += 1;
        }
        from = abs + needle.len();
    }
    count
}

/// Split `lines` into logical statements for the splice-window analysis. A
/// logical statement accumulates characters until a `;` or a block-closing `}`
/// is reached at bracket depth zero; the accumulated text (interior newlines
/// preserved) is one window. This lets [`splices_built_output`] see a
/// `format!`/concat rejoin that rustfmt has wrapped across several physical
/// lines as ONE expression instead of several single-slice lines.
///
/// Bracket depth tracks `()`, `[]`, and `{}`; `"…"` string and `'…'` char
/// literals are skipped so a stray delimiter or `;` inside a literal cannot
/// mis-split a statement (a lifetime `'a` is left untouched — only a closed
/// char literal opens skip mode). Depth is clamped at zero, so the first `}`
/// that closes the binding's enclosing block still terminates the trailing
/// statement. Comments are already removed by [`strip_comment_lines`].
fn logical_statements(lines: &[&str]) -> Vec<String> {
    let mut statements = Vec::new();
    let mut current = String::new();
    let mut depth: i32 = 0;
    let mut in_string = false;
    let mut in_char = false;
    let mut escaped = false;

    for (line_index, line) in lines.iter().enumerate() {
        if line_index > 0 {
            current.push('\n');
        }
        let chars: Vec<char> = line.chars().collect();
        let mut j = 0;
        while j < chars.len() {
            let c = chars[j];
            current.push(c);
            j += 1;

            if in_string {
                if escaped {
                    escaped = false;
                } else if c == '\\' {
                    escaped = true;
                } else if c == '"' {
                    in_string = false;
                }
                continue;
            }
            if in_char {
                if escaped {
                    escaped = false;
                } else if c == '\\' {
                    escaped = true;
                } else if c == '\'' {
                    in_char = false;
                }
                continue;
            }

            match c {
                '"' => in_string = true,
                '\'' => {
                    // Distinguish a char literal (`'x'`, `'\n'`), which opens
                    // skip mode, from a lifetime (`'a`), which does not. `j`
                    // already points past the opening quote.
                    let is_escape_literal = chars.get(j) == Some(&'\\');
                    let is_simple_literal = chars.get(j + 1) == Some(&'\'');
                    if is_escape_literal || is_simple_literal {
                        in_char = true;
                    }
                }
                '(' | '[' | '{' => depth += 1,
                ')' | ']' | '}' => {
                    if depth > 0 {
                        depth -= 1;
                    }
                    if depth == 0 && c == '}' {
                        statements.push(std::mem::take(&mut current));
                    }
                }
                ';' if depth == 0 => statements.push(std::mem::take(&mut current)),
                _ => {}
            }
        }
    }

    if !current.trim().is_empty() {
        statements.push(current);
    }
    statements
}

/// True if `stmt` reassembles the built binding `ident` from two or more slices
/// — the canonical cut-and-rejoin splice, whether joined by `format!` or `+`:
/// `format!("{}{}", &b[..a], &b[c..])` or `b[..a].to_string() + &b[c..]`. `stmt`
/// is one LOGICAL statement (see [`logical_statements`]), so a rejoin that
/// rustfmt has wrapped across several physical lines — one `&b[..]` fragment per
/// line — is still counted as one window. Both splice shapes rejoin two slices
/// of the built output, so keying on a slice COUNT (never a bare `+`, which also
/// appears inside index arithmetic like `&b[..n + 1]`, nor a lone `format!`,
/// which also wraps a single-fragment debug log) keeps the detector free of
/// false positives: a single read-only slice — even inside a multi-line
/// `format!` — is legitimate consumption and never fires.
fn splices_built_output(stmt: &str, ident: &str) -> bool {
    count_built_slices(stmt, ident) >= 2
}

/// Detect post-`build_string()` string surgery in `src`. Returns a list of
/// human-readable violation descriptions (empty == clean).
fn detect_violations(src: &str) -> Vec<String> {
    let code = strip_comment_lines(src);
    let mut violations = Vec::new();

    // (1) Direct chain: `build_string().<surgery>(`.
    let needle = "build_string()";
    let mut search_from = 0;
    while let Some(rel) = code[search_from..].find(needle) {
        let end_of_call = search_from + rel + needle.len();
        if chain_after_is_surgery(&code, end_of_call) {
            violations.push(format!(
                "`build_string()` result is mutated by a directly-chained string-surgery \
                 method near byte offset {end_of_call}"
            ));
        }
        search_from = end_of_call;
    }

    // (2) Bound-then-mutated: `let s = …build_string();` then `s.<surgery>(`.
    let lines: Vec<&str> = code.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        if !line.contains(needle) {
            continue;
        }
        let Some(ident) = binding_ident(line) else {
            continue;
        };
        // Scan the remainder of the file for surgery on this binding.
        for follow in lines.iter().skip(i + 1) {
            // Receiver surgery: `built.replace(…)` / `.replace_range(…)` / ….
            for method in SURGERY_METHODS {
                let pat = format!("{ident}.{method}(");
                if follow.contains(&pat) {
                    violations.push(format!(
                        "binding `{ident}` (assigned from `build_string()` at line {}) is \
                         mutated by `.{method}(` — post-codegen string surgery",
                        i + 1
                    ));
                }
            }
            // Regex / argument replacement: built output is the haystack of a
            // `replace`-family call, e.g. `re.replace_all(&built, …)`.
            if replace_takes_built_haystack(follow, ident) {
                violations.push(format!(
                    "binding `{ident}` (assigned from `build_string()` at line {}) is rewritten \
                     by a regex/`replace`-family call taking it as the haystack — post-codegen \
                     string surgery",
                    i + 1
                ));
            }
        }
        // Manual splice: built output sliced and reassembled into a new string
        // via concatenation or `format!`. The rejoin is a single expression that
        // rustfmt may wrap across several physical lines (one `&built[..]`
        // fragment per line), so the splice window is a LOGICAL statement, not a
        // single follow line — a per-line slice count never reaches two on that
        // rustfmt-default shape.
        for stmt in logical_statements(&lines[i + 1..]) {
            if splices_built_output(&stmt, ident) {
                violations.push(format!(
                    "binding `{ident}` (assigned from `build_string()` at line {}) is sliced and \
                     reassembled (manual splice/rebuild) — post-codegen string surgery",
                    i + 1
                ));
            }
        }
    }

    violations
}

/// The production sweep: no `verter_compiler` production source mutates a
/// `build_string()` result via string surgery.
#[test]
fn no_post_codegen_string_surgery() {
    let src_dir = crate_root().join("src");
    let mut files = Vec::new();
    collect_production_rs_files(&src_dir, &mut files);
    assert!(
        !files.is_empty(),
        "scanner found no production .rs files under {} — the walk is broken",
        src_dir.display()
    );

    let mut findings: Vec<String> = Vec::new();
    for file in &files {
        let src = match std::fs::read_to_string(file) {
            Ok(s) => s,
            Err(_) => continue,
        };
        for v in detect_violations(&src) {
            let rel = file.strip_prefix(crate_root()).unwrap_or(file);
            findings.push(format!("{}: {v}", rel.display()));
        }
    }

    assert!(
        findings.is_empty(),
        "CodeTransform SSOT — production code under crates/verter_compiler/src must NOT \
         rewrite a `build_string()` result via receiver surgery (`.replace(` / `.replacen(` / \
         `.replace_range(` / `.insert_str(`), a regex/`replace`-family call taking it as the \
         haystack (`re.replace_all(&built, …)`), or a manual slice-and-rejoin splice \
         (`format!(\"{{}}{{}}\", &built[..a], &built[b..])`). String surgery on built output \
         desyncs source-map byte offsets. Route the change through a `CodeTransform` operation \
         (overwrite / prepend_left / append_left / the format-sink emission APIs) BEFORE \
         `build_string()` instead. Violations:\n  {}",
        findings.join("\n  ")
    );
}

/// Self-discrimination: the detector MUST fire on a directly-chained surgery.
#[test]
fn detects_direct_chain_surgery() {
    let bad = r#"
        fn build_tsx(ct: CodeTransform) -> String {
            ct.build_string().replace(".vue'", ".vue.ts'")
        }
    "#;
    assert!(
        !detect_violations(bad).is_empty(),
        "detector failed to flag a directly-chained `build_string().replace(...)` — too permissive"
    );
}

/// Self-discrimination: the detector MUST fire on a bound-then-mutated result.
#[test]
fn detects_bound_then_mutated_surgery() {
    let bad = r#"
        fn build_tsx(ct: CodeTransform) -> String {
            let tsx = ct.build_string();
            let fixed = tsx.replace(".vue'", ".vue.ts'");
            fixed
        }
    "#;
    assert!(
        !detect_violations(bad).is_empty(),
        "detector failed to flag a bound-then-mutated `build_string()` result — too permissive"
    );
}

/// Self-discrimination: regex replacement of the built output — the binding is
/// the HAYSTACK of a `Regex::replace_all` — is post-codegen surgery.
#[test]
fn detects_regex_replace_of_built_output() {
    let bad = r#"
        fn rewrite(ct: CodeTransform, re: &Regex) -> String {
            let tsx = ct.build_string();
            let fixed = re.replace_all(&tsx, "X");
            fixed.into_owned()
        }
    "#;
    assert!(
        !detect_violations(bad).is_empty(),
        "detector failed to flag `re.replace_all(&built, …)` rewriting the built output — too \
         permissive"
    );
}

/// Self-discrimination: manually slicing the built output and rejoining the
/// pieces — via `+` concatenation or a `format!` rebuild — is post-codegen
/// surgery (it desyncs source-map byte offsets just like `.replace`).
#[test]
fn detects_manual_splice_of_built_output() {
    let bad_concat = r#"
        fn splice(ct: CodeTransform) -> String {
            let built = ct.build_string();
            built[..10].to_string() + &built[20..]
        }
    "#;
    assert!(
        !detect_violations(bad_concat).is_empty(),
        "detector failed to flag a slice+concat splice of the built output — too permissive"
    );

    let bad_format = r#"
        fn splice(ct: CodeTransform) -> String {
            let built = ct.build_string();
            format!("{}{}", &built[..10], &built[20..])
        }
    "#;
    assert!(
        !detect_violations(bad_format).is_empty(),
        "detector failed to flag a format!-rebuild splice of the built output — too permissive"
    );
}

/// Self-discrimination: a `format!` slice-and-rejoin splice that rustfmt has
/// wrapped across several lines — ONE built slice per physical line — must still
/// be flagged. A line-local slice count never reaches two here (each
/// `&built[..]` argument sits on its own line), so the splice window must be a
/// logical statement, not a physical line. This is the canonical rustfmt-default
/// shape of the very anti-pattern [`detects_manual_splice_of_built_output`]
/// covers on a single line.
#[test]
fn detects_multiline_format_splice_of_built_output() {
    let bad = r#"
        fn splice(ct: CodeTransform) -> String {
            let built = ct.build_string();
            let out = format!(
                "{}{}",
                &built[..10],
                &built[20..],
            );
            out
        }
    "#;
    assert!(
        !detect_violations(bad).is_empty(),
        "detector failed to flag a multi-line `format!` slice-and-rejoin splice (one built slice \
         per line) — the splice window must be a logical statement, not a physical line"
    );
}

/// Self-discrimination: the slice-and-rejoin window is a logical statement even
/// when the rejoin is `+` concatenation wrapped across lines, e.g.
/// `built[..a].to_string()\n    + &built[b..];`. Like the `format!` case above,
/// each slice lands on its own physical line, so a line-local count misses it.
#[test]
fn detects_multiline_concat_splice_of_built_output() {
    let bad = r#"
        fn splice(ct: CodeTransform) -> String {
            let built = ct.build_string();
            let out = built[..10].to_string()
                + &built[20..];
            out
        }
    "#;
    assert!(
        !detect_violations(bad).is_empty(),
        "detector failed to flag a multi-line `+`-concat slice-and-rejoin splice — the splice \
         window must be a logical statement, not a physical line"
    );
}

/// Self-discrimination (negative): a multi-line `format!` that consumes a SINGLE
/// built slice — the call wrapped across lines by rustfmt — is read-only
/// consumption, not a rejoin, and must NOT be flagged. This pins that the
/// statement-window splice detector still keys on the slice COUNT: one slice
/// never fires, however many lines the expression spans.
#[test]
fn accepts_multiline_format_with_single_built_slice() {
    let good = r#"
        fn render(ct: CodeTransform) -> String {
            let built = ct.build_string();
            let s = format!(
                "prefix {} suffix",
                &built[..2],
            );
            s
        }
    "#;
    assert!(
        detect_violations(good).is_empty(),
        "detector false-positived on a multi-line `format!` consuming a single built slice: {:?}",
        detect_violations(good)
    );
}

/// Self-discrimination (negative): two SEPARATE read-only slices of the built
/// output, each in its own `;`-terminated statement, are not a rejoin and must
/// NOT be flagged — even though both reference the built binding. This pins that
/// statement windowing splits on statement boundaries and does not accumulate
/// slices across independent statements into a phantom splice.
#[test]
fn accepts_separate_single_slice_statements() {
    let good = r#"
        fn inspect(ct: CodeTransform) -> usize {
            let built = ct.build_string();
            let head = &built[..2];
            let tail = &built[5..];
            head.len() + tail.len()
        }
    "#;
    assert!(
        detect_violations(good).is_empty(),
        "detector false-positived on two separate single-slice statements: {:?}",
        detect_violations(good)
    );
}

/// Self-discrimination (negative): legitimate `build_string()` consumption —
/// binding the result and returning/storing it, or comparing it — must NOT be
/// flagged. Without this, an over-broad scanner that flags every
/// `build_string()` would pass the production sweep vacuously today and block
/// all future codegen edits.
#[test]
fn accepts_clean_codegen_usage() {
    let good = r#"
        fn emit(ct: CodeTransform) -> CompileResult {
            let tsx_code = ct.build_string();
            CompileResult { code: tsx_code }
        }
        fn other(ct: CodeTransform) -> String {
            ct.build_string()
        }
    "#;
    assert!(
        detect_violations(good).is_empty(),
        "detector false-positived on legitimate build_string() consumption: {:?}",
        detect_violations(good)
    );
}

/// Self-discrimination (negative): a single read-only slice of the built output
/// — reading a fragment for an assertion or a debug `format!`, with NO rejoin —
/// is legitimate consumption and must NOT be flagged. Only a cut-and-rejoin
/// (two+ slices) is a splice; this pins that the splice detector keys on the
/// slice COUNT, not the mere presence of a slice or a `format!`.
#[test]
fn accepts_readonly_slice_of_built_output() {
    let good = r#"
        fn inspect(ct: CodeTransform) -> usize {
            let built = ct.build_string();
            debug_assert!(format!("head: {}", &built[..2]).len() > 0);
            built[..1].len()
        }
    "#;
    assert!(
        detect_violations(good).is_empty(),
        "detector false-positived on a single read-only slice of the built output: {:?}",
        detect_violations(good)
    );
}

/// Comment-only "don't do this" examples inside doc comments must not trip the
/// scanner (the comment-stripping pre-pass guarantees this).
#[test]
fn ignores_surgery_in_comment_examples() {
    let doc_example = r#"
        /// WRONG: do not do `ct.build_string().replace(".vue'", ".vue.ts'")`.
        // let s = ct.build_string(); s.replace("a", "b");
        fn real(ct: CodeTransform) -> String {
            ct.build_string()
        }
    "#;
    assert!(
        detect_violations(doc_example).is_empty(),
        "detector flagged a surgery pattern that only appears inside comments: {:?}",
        detect_violations(doc_example)
    );
}
