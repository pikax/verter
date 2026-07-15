//! Shared test-support for the Svelte CSS architecture guards: the ONE
//! string-aware Rust-source comment scanner every guard's verdict predicate
//! strips through.
//!
//! A guard that scans production source for banned tokens must remove
//! comments (so a doc-comment mention is not a violation) WITHOUT treating a
//! `//` or `/*` inside a string/char literal as a comment start — a naive
//! stripper swallows the rest of the line (or everything to a distant `*/`)
//! and HIDES a same-line banned token from the guard (a false negative:
//! `let url = "http://x"; crate::css::process_style(url)` would pass). This
//! module is the single shared implementation; no guard keeps a private
//! copy. The discrimination suite below runs inside every including guard
//! binary.
//!
//! Lives under the consolidated test binary at
//! `tests/cases/svelte_guard_support/mod.rs`. `tests/cases/mod.rs` loads it
//! once, and each guard imports that single shared module.

/// Strip `//` line comments and `/* … */` block comments so a guard scan
/// keys on real code, not a token mentioned in a doc comment. (String-literal
/// content is kept — a banned token inside a string constant is still a
/// smell worth failing on.)
///
/// STRING-AWARE over the FULL Rust string-kind class: a `//` or `/*` INSIDE a
/// string literal (`"http://x"`), a raw string (`r"a/*b"`, `r#"…"#`, N-hash),
/// a byte / raw-byte string (`b"…"`, `br"…"`), a C-string / raw C-string
/// (`c"…"`, `cr"…"`, `cr#"…"#`), or a char literal is NOT a comment start — a
/// naive stripper would swallow the rest of the line (or everything to a
/// distant `*/`) and HIDE a same-line banned token from the guard (a false
/// negative). Kind matters: raw kinds are NOT escape-aware (a `cr"tail\"`
/// closes at the quote after the backslash), escape-aware kinds (`"…"`,
/// `b"…"`, `c"…"`) honor `\"`. String CONTENT is copied through verbatim;
/// only genuine comments are removed.
pub fn strip_rust_comments(src: &str) -> String {
    let chars: Vec<char> = src.chars().collect();
    let n = chars.len();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    while i < n {
        let c = chars[i];
        let next = if i + 1 < n { chars[i + 1] } else { '\0' };
        // Genuine comment starts (we are OUTSIDE any literal here).
        if c == '/' && next == '/' {
            while i < n && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if c == '/' && next == '*' {
            i += 2;
            while i < n && !(chars[i] == '*' && i + 1 < n && chars[i + 1] == '/') {
                i += 1;
            }
            i += 2;
            continue;
        }
        // A RAW (optionally byte / C) string literal: `r"…"` / `r#"…"#` /
        // `br##"…"##` / `cr"…"` / `cr#"…"#` — N hashes open, the SAME N
        // hashes close; no escapes inside. Require a non-identifier
        // predecessor so an identifier ending in `r`/`br`/`cr` directly
        // followed by `"` does not lex as a raw-string opener.
        let prev_is_ident = i > 0 && (chars[i - 1].is_alphanumeric() || chars[i - 1] == '_');
        if !prev_is_ident && (c == 'r' || ((c == 'b' || c == 'c') && next == 'r')) {
            let mut j = i + if c == 'r' { 1 } else { 2 };
            let mut hashes = 0;
            while j < n && chars[j] == '#' {
                hashes += 1;
                j += 1;
            }
            if j < n && chars[j] == '"' {
                // Copy the opener verbatim.
                for &ch in &chars[i..=j] {
                    out.push(ch);
                }
                i = j + 1;
                // Copy content until `"` followed by `hashes` `#`s.
                'raw: while i < n {
                    if chars[i] == '"' {
                        let mut k = 0;
                        while k < hashes && i + 1 + k < n && chars[i + 1 + k] == '#' {
                            k += 1;
                        }
                        if k == hashes {
                            for &ch in &chars[i..=i + hashes] {
                                out.push(ch);
                            }
                            i += hashes + 1;
                            break 'raw;
                        }
                    }
                    out.push(chars[i]);
                    i += 1;
                }
                continue;
            }
            // Not a raw string (`r` starts an identifier) — fall through.
        }
        // A normal (optionally byte / C) string literal with `\` escapes —
        // `"…"` / `b"…"` / `c"…"` (the C-string kind is escape-aware like a
        // normal string).
        if c == '"' || ((c == 'b' || c == 'c') && next == '"' && !prev_is_ident) {
            if c != '"' {
                out.push(c);
                i += 1;
            }
            out.push('"');
            i += 1;
            while i < n {
                if chars[i] == '\\' && i + 1 < n {
                    out.push(chars[i]);
                    out.push(chars[i + 1]);
                    i += 2;
                    continue;
                }
                let ch = chars[i];
                out.push(ch);
                i += 1;
                if ch == '"' {
                    break;
                }
            }
            continue;
        }
        // A char literal (`'"'` / `'\\''` / `'/'`) — so a quote or slash inside
        // one cannot open a string / comment. DISAMBIGUATED from a lifetime
        // (`'a`): a lifetime is `'` + ident-start NOT followed by a closing
        // `'` (a char literal `'a'` has the closing quote right after).
        if c == '\'' {
            if next == '\\' {
                // An escaped char literal: copy `'\x'` (escape + payload + `'`).
                out.push(c);
                i += 1;
                if i < n {
                    out.push(chars[i]); // the backslash
                    i += 1;
                }
                while i < n {
                    let ch = chars[i];
                    out.push(ch);
                    i += 1;
                    if ch == '\'' {
                        break;
                    }
                }
                continue;
            }
            let after_payload = if i + 2 < n { chars[i + 2] } else { '\0' };
            if next != '\0' && after_payload == '\'' && next != '\'' {
                // A plain single-char literal `'x'` (including `'"'` / `'/'`).
                out.push(c);
                out.push(next);
                out.push('\'');
                i += 3;
                continue;
            }
            // A lifetime (`'a`) or a lone quote — copied through, opens nothing.
        }
        out.push(c);
        i += 1;
    }
    out
}

// ─────────────── the shared scanner's own discrimination suite ───────────────
//
// Runs inside every guard binary that includes this module. Each case pins
// one string-kind × comment-lookalike combination in BOTH directions: a
// banned-token lookalike stays VISIBLE (no false negative) and a genuine
// comment still strips (no false positive).

/// The bypass probe the suite keys on (any recognizable banned-ish token
/// works — the guards apply their own token lists on the stripped output).
#[cfg(test)]
const PROBE: &str = "crate::css::process_style";

#[test]
fn scanner_keeps_the_exact_bypass_visible_and_strips_the_genuine_comment() {
    // THE bypass: a `//` inside an ordinary string literal must not swallow
    // the same-line banned call.
    let bypass = r#"fn f() { let url = "http://x"; crate::css::process_style(url, true); }"#;
    assert!(
        strip_rust_comments(bypass).contains(PROBE),
        "the banned call after a string-embedded `//` must stay visible"
    );
    // A genuine `// crate::css::process_style` COMMENT is ignored.
    assert!(
        !strip_rust_comments("// crate::css::process_style\nfn ok() {}").contains(PROBE),
        "a genuine comment mention must strip"
    );
    // A genuine comment AFTER a string on the same line still strips.
    assert!(
        !strip_rust_comments(r#"fn f() { let url = "http://x"; } // crate::css::process_style"#)
            .contains(PROBE),
        "a genuine comment after a string must still strip"
    );
}

#[test]
fn scanner_handles_ordinary_strings_with_escapes() {
    // An ESCAPED quote does not close the string early — the `//` after it is
    // still string content, so the later call stays visible.
    let src = r#"fn f() { let s = "a\" // b"; crate::css::process_style(s, true); }"#;
    assert!(strip_rust_comments(src).contains(PROBE));
    // String CONTENT is kept verbatim (a token inside a string still scans).
    assert!(strip_rust_comments(r#"let name = "crate::css::process_style";"#).contains(PROBE));
}

#[test]
fn scanner_handles_raw_strings() {
    // A `/*` inside a raw string must not eat to a distant `*/` (or EOF).
    let src = r##"fn f() { let s = r"a/*b"; crate::css::process_style(s, true); }"##;
    assert!(strip_rust_comments(src).contains(PROBE));
    // The hashed raw form with an embedded `//`.
    let src = "fn f() { let s = r#\"see // here\"#; crate::css::process_style(s, true); }";
    assert!(strip_rust_comments(src).contains(PROBE));
    // Raw content is NOT escape-aware: `r"tail\"` closes at the quote after
    // the backslash; the following genuine comment still strips.
    let src = "fn f() { let s = r\"tail\\\"; } // crate::css::process_style";
    assert!(!strip_rust_comments(src).contains(PROBE));
}

#[test]
fn scanner_handles_byte_strings() {
    // A plain byte string with an embedded `//`.
    let src = "fn f() { let url = b\"http://x\"; crate::css::process_style(url, true); }";
    assert!(strip_rust_comments(src).contains(PROBE));
    // A raw byte string with an embedded `/*`.
    let src = "fn f() { let s = br#\"a/*b\"#; crate::css::process_style(s, true); }";
    assert!(strip_rust_comments(src).contains(PROBE));
    // A genuine comment after a byte string still strips.
    let src = "fn f() { let url = b\"http://x\"; } // crate::css::process_style";
    assert!(!strip_rust_comments(src).contains(PROBE));
}

#[test]
fn scanner_handles_c_strings() {
    // A plain C-string with an embedded `//` (escape-aware kind).
    let src = "fn f() { let url = c\"http://x\"; crate::css::process_style(url, true); }";
    assert!(strip_rust_comments(src).contains(PROBE));
    // An escaped quote inside a C-string does not close it early.
    let src = "fn f() { let s = c\"a\\\" // b\"; crate::css::process_style(s, true); }";
    assert!(strip_rust_comments(src).contains(PROBE));
}

#[test]
fn scanner_handles_raw_c_strings() {
    // A HASHED raw C-string may contain a BARE `"` — a scanner that lexes it
    // as a normal string closes at the embedded quote and treats the
    // following `//` as a comment start, hiding the same-line call.
    let src =
        "fn f() { let s = cr#\"quote \" then // lookalike\"#; crate::css::process_style(s, true); }";
    assert!(strip_rust_comments(src).contains(PROBE));
    // A raw C-string with an embedded `/*` must not eat to a distant `*/`.
    let src = "fn f() { let s = cr\"a/*b\"; crate::css::process_style(s, true); }";
    assert!(strip_rust_comments(src).contains(PROBE));
    // Raw-C content is NOT escape-aware: `cr"tail\"` closes at the quote
    // right after the backslash — the following genuine comment still
    // strips (the false-positive direction of the kind confusion).
    let src = "fn f() { let s = cr\"tail\\\"; } // crate::css::process_style";
    assert!(!strip_rust_comments(src).contains(PROBE));
    // An identifier ending in `cr` followed by `"` is NOT a raw C-string
    // opener — the string after it is plain and the genuine comment strips.
    let src = "fn f(macr: &str) { let s = \"x\"; let _ = (macr, s); } // crate::css::process_style";
    assert!(!strip_rust_comments(src).contains(PROBE));
}

#[test]
fn scanner_handles_char_literals_and_lifetimes() {
    // A `'/'` char literal must not open anything that hides a later call.
    let src = "fn f() { let c = '/'; crate::css::process_style(\"x\", true); }";
    assert!(strip_rust_comments(src).contains(PROBE));
    // An escaped char literal (`'\''`) is consumed as one literal.
    let src = "fn f() { let q = '\\''; crate::css::process_style(\"x\", true); }";
    assert!(strip_rust_comments(src).contains(PROBE));
    // A `'"'` char literal must not open a string that swallows the rest.
    let src = "fn f() { let q = '\"'; } // crate::css::process_style";
    assert!(!strip_rust_comments(src).contains(PROBE));
    // A lifetime does NOT open a char-literal state — the genuine comment
    // after it still strips.
    let src = "fn f<'a>(x: &'a str) -> &'a str { x } // crate::css::process_style";
    assert!(!strip_rust_comments(src).contains(PROBE));
}
