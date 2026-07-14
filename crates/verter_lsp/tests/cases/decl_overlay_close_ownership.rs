//! Architecture guard: the declaration-overlay close is owned by ONE authority.
//!
//! A `.d.<ext>.ts` declaration overlay's provider close (`close_dts`) is issued by
//! EXACTLY ONE place — the declaration-overlay lifecycle owner
//! (`DeclOverlayOwner`, in `src/background_drain_decl_closure.rs`). The owner
//! serializes the close behind the overlay's per-path lock and re-checks the
//! overlay's reachability + close generation before the destructive close, so a
//! stale close can never clobber a concurrent open of the same overlay (which would
//! strand an open carrier root's bare import on TS2307) and a closed root can never
//! resurrect an overlay no live root reaches.
//!
//! The generic stale-path closers (`background_drain::close_stale_provider_paths`,
//! `sync_coordinator::close_stale_paths`, `workspace_scanner::close_stale_paths`)
//! and the provider-state close dispatch must NEVER issue a raw `close_dts` for a
//! `Decl`-classified path. The type split (`NonDeclProviderPathKind`) already makes
//! that a compile-time impossibility at the generic closers; THIS guard is the
//! belt-and-suspenders backstop against the raw source pattern reappearing anywhere
//! in production LSP source: a function that classifies a `ProviderPathKind::Decl`
//! path and issues a `close_dts` for it without delegating to the owner (the exact
//! "stray unguarded Decl close" footgun).
//!
//! Detection is PER CALL SITE over a CODE-ONLY view of each function (comments —
//! including NESTED block comments — and string/char/raw-string/template bodies
//! blanked, while Rust lifetimes and labels stay code, so a token buried in prose
//! never triggers nor exempts an offense, and a `{`/`}` inside a string/comment
//! cannot mislead the brace matcher). For each raw `close_dts(` call the guard reads
//! its governing discriminant — the nearest `ProviderPathKind::…` token textually
//! before it: a close governed by a non-`Decl` kind (`Api`/`Ide`/`Shadow`) serves a
//! non-Decl arm and is safe; a close governed by `ProviderPathKind::Decl` (or
//! reachable from a `Decl` classification with no nearer non-`Decl` kind) is the
//! footgun. The single SAFE outside-owner shape is
//! `provider_state::close_provider_paths`, whose `Decl` arm DELEGATES to
//! `guarded_close` and whose only raw `close_dts` is on the non-`Decl` `Api` arm.
//!
//! The sanctioned delegation is the `guarded_close(` CALL ITSELF — and that call is
//! not a raw `close_dts(`. A delegation grants NO blanket exemption to any other raw
//! close: every raw `close_dts(` reachable from a `Decl` classification is an offense
//! regardless of a preceding `guarded_close(`, so a region that delegates AND THEN
//! also raw-closes is flagged on the stray close, and a function that delegates in
//! one branch but strays in another is flagged on the stray branch. The lone safe
//! pairing is a raw close fenced to a more-recent non-`Decl` kind (the `Api` arm of
//! the delegating dispatch).
//!
//! Discriminates: against a tree where any non-owner function issues a raw
//! `close_dts(` reachable from a `ProviderPathKind::Decl` classification (and not
//! fenced to a nearer non-`Decl` kind), this guard FAILS. With every Decl close
//! routed through the owner's `guarded_close`, it PASSES — only the owner module
//! (and test files) may pair `Decl` with a provider close.

use std::path::{Path, PathBuf};

/// The LSP crate's `src` root.
fn src_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// The ONLY production module permitted to pair `ProviderPathKind::Decl` with a
/// declaration-overlay provider close — the lifecycle owner. Its file basename.
const OWNER_MODULE_BASENAME: &str = "background_drain_decl_closure.rs";

/// Strip the trailing `#[cfg(test)] mod tests { .. }` (and anything after the first
/// `#[cfg(test)]`) so the guard scans PRODUCTION source only — an inline test module
/// is allowed to construct `(ProviderPathKind::Decl, _)` close targets.
fn production_only(src: &str) -> String {
    match src.find("\n#[cfg(test)]") {
        Some(idx) => src[..idx].to_string(),
        None => src.to_string(),
    }
}

/// Recursively collect every `.rs` file under `dir`, EXCLUDING the owner module and
/// any extracted `*_tests.rs` test file (a test may legitimately name a Decl close
/// target). Returns `(relative_label, absolute_path)` pairs.
fn collect_production_rs_files(dir: &Path, root: &Path, out: &mut Vec<(String, PathBuf)>) {
    let entries = std::fs::read_dir(dir).unwrap_or_else(|e| panic!("read_dir {dir:?}: {e}"));
    for entry in entries {
        let entry = entry.expect("dir entry");
        let path = entry.path();
        if path.is_dir() {
            collect_production_rs_files(&path, root, out);
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !name.ends_with(".rs") {
            continue;
        }
        // The owner module is the single allowed home of a Decl provider close.
        if name == OWNER_MODULE_BASENAME {
            continue;
        }
        // Extracted test files (`*_tests.rs`) may construct Decl close targets.
        if name.ends_with("_tests.rs") {
            continue;
        }
        let rel = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        out.push((rel, path));
    }
}

/// A CODE-ONLY view of `src`: every comment body (`//…`, `/* … */`) and every
/// string / char / template-literal body is replaced by spaces, with newlines and
/// byte offsets PRESERVED (so line numbers and slice positions still map 1:1 onto
/// the original). The delimiters themselves are blanked too — what survives is the
/// code skeleton. Scanning this view means a token like `close_dts(`,
/// `guarded_close(`, or `ProviderPathKind::Decl` that appears only inside a comment
/// or a string literal VANISHES (so it neither triggers nor falsely exempts an
/// offense), and a `{` / `}` inside a string or comment cannot mislead the brace
/// matcher into under- or over-extending a function body.
///
/// This is a focused lexer over the Rust lexical states the structural scan must
/// see through — line comment, NESTED block comment, raw string, normal/byte
/// string, char literal (distinguished from a lifetime/label), and (for the shared
/// cross-language source) a JS/TS template literal — not a full Rust parser, but
/// sound enough to keep the guard's structural scan honest about what is code and
/// what is text.
fn code_only(src: &str) -> String {
    let bytes = src.as_bytes();
    let len = bytes.len();
    let mut out: Vec<u8> = Vec::with_capacity(len);
    // Blank a byte in the output (preserving newlines so line numbers are stable).
    let blank = |out: &mut Vec<u8>, b: u8| out.push(if b == b'\n' { b'\n' } else { b' ' });
    let mut i = 0usize;
    while i < len {
        let b = bytes[i];

        // Line comment: blank through end of line (keep the newline).
        if b == b'/' && i + 1 < len && bytes[i + 1] == b'/' {
            while i < len && bytes[i] != b'\n' {
                blank(&mut out, bytes[i]);
                i += 1;
            }
            continue;
        }
        // Block comment: Rust block comments NEST, so track depth and blank through
        // the MATCHING `*/` (an inner `*/` only drops one level). `/* /* */ */` is
        // fully blanked; a real offense after the OUTER `*/` is still seen.
        if b == b'/' && i + 1 < len && bytes[i + 1] == b'*' {
            let mut depth = 0usize;
            while i + 1 < len {
                if bytes[i] == b'/' && bytes[i + 1] == b'*' {
                    blank(&mut out, bytes[i]);
                    blank(&mut out, bytes[i + 1]);
                    i += 2;
                    depth += 1;
                    continue;
                }
                if bytes[i] == b'*' && bytes[i + 1] == b'/' {
                    blank(&mut out, bytes[i]);
                    blank(&mut out, bytes[i + 1]);
                    i += 2;
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                    continue;
                }
                blank(&mut out, bytes[i]);
                i += 1;
            }
            // Unterminated comment: blank any trailing byte so nothing leaks as code.
            if depth != 0 {
                while i < len {
                    blank(&mut out, bytes[i]);
                    i += 1;
                }
            }
            continue;
        }
        // Raw string: `r"…"`, `r#"…"#`, `r##"…"##`, and the byte forms `br"…"` /
        // `br#"…"#` — the closing delimiter is `"` followed by the SAME number of
        // `#` as the opener, so an embedded `"` (or `}`/`)`) inside the body cannot
        // close it early and desync the scan. The `r`/`b` must be at a token
        // boundary (the previous byte is not an identifier byte) so the `r` in
        // `error`/`for` is not mistaken for a raw-string prefix.
        {
            let prev_is_ident = i > 0 && is_ident_byte(bytes[i - 1]);
            let after_prefix = if !prev_is_ident && b == b'r' {
                Some(i + 1)
            } else if !prev_is_ident && b == b'b' && i + 1 < len && bytes[i + 1] == b'r' {
                Some(i + 2)
            } else {
                None
            };
            if let Some(after_r) = after_prefix {
                let mut j = after_r;
                let mut hashes = 0usize;
                while j < len && bytes[j] == b'#' {
                    hashes += 1;
                    j += 1;
                }
                if j < len && bytes[j] == b'"' {
                    // Confirmed raw-string opener — blank the prefix and opening `"`.
                    while i <= j {
                        blank(&mut out, bytes[i]);
                        i += 1;
                    }
                    // Blank the body up to and including the matching `"` + `#`*hashes.
                    loop {
                        if i >= len {
                            break;
                        }
                        if bytes[i] == b'"' {
                            let mut k = i + 1;
                            let mut seen = 0usize;
                            while k < len && seen < hashes && bytes[k] == b'#' {
                                seen += 1;
                                k += 1;
                            }
                            if seen == hashes {
                                while i < k {
                                    blank(&mut out, bytes[i]);
                                    i += 1;
                                }
                                break;
                            }
                        }
                        blank(&mut out, bytes[i]);
                        i += 1;
                    }
                    continue;
                }
            }
        }
        // Template literal: blank its body, including `${ … }` interpolations (the
        // guard does not need to see code inside an interpolation), across lines.
        // An escaped backtick does not close the template.
        if b == b'`' {
            blank(&mut out, b);
            i += 1;
            while i < len {
                let c = bytes[i];
                if c == b'\\' {
                    blank(&mut out, c);
                    if i + 1 < len {
                        blank(&mut out, bytes[i + 1]);
                    }
                    i += 2;
                    continue;
                }
                if c == b'`' {
                    blank(&mut out, c);
                    i += 1;
                    break;
                }
                blank(&mut out, c);
                i += 1;
            }
            continue;
        }
        // Char literal vs lifetime/label. A `'` opens a CHAR LITERAL only if it is a
        // `'\…'` escape or a `'x'` single char then a closing quote; otherwise it is
        // a LIFETIME or label (`'a`, `'static`, `'_`, `'outer:`) whose `'` is
        // ordinary code and must NOT open a string span (which would blank from the
        // apostrophe to the next `'`/EOF and could hide a later offense).
        if b == b'\'' {
            let is_char_literal =
                (i + 1 < len && bytes[i + 1] == b'\\') || (i + 2 < len && bytes[i + 2] == b'\'');
            if is_char_literal {
                let quote = b;
                blank(&mut out, b);
                i += 1;
                while i < len {
                    let c = bytes[i];
                    if c == b'\\' {
                        blank(&mut out, c);
                        if i + 1 < len {
                            blank(&mut out, bytes[i + 1]);
                        }
                        i += 2;
                        continue;
                    }
                    if c == quote {
                        blank(&mut out, c);
                        i += 1;
                        break;
                    }
                    blank(&mut out, c);
                    i += 1;
                }
                continue;
            }
            // Lifetime / label: fall through to copy the apostrophe as code.
        }
        // String / byte-string literal: blank its body, consuming escape pairs.
        if b == b'"' {
            let quote = b;
            blank(&mut out, b);
            i += 1;
            while i < len {
                let c = bytes[i];
                if c == b'\\' {
                    blank(&mut out, c);
                    if i + 1 < len {
                        blank(&mut out, bytes[i + 1]);
                    }
                    i += 2;
                    continue;
                }
                if c == quote {
                    blank(&mut out, c);
                    i += 1;
                    break;
                }
                blank(&mut out, c);
                i += 1;
            }
            continue;
        }

        out.push(b);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_default()
}

/// A byte that may appear inside a Rust identifier (so a leading `r`/`b` that is
/// merely the tail of an identifier like `for`/`error` is NOT a raw-string prefix).
fn is_ident_byte(b: u8) -> bool {
    b == b'_' || b.is_ascii_alphanumeric()
}

/// The `fn`-introduced function bodies of the CODE-ONLY view of `src`, each as
/// `(start_line_1based, code_only_body_text)`. A body spans from the `fn`'s
/// signature through its brace-balanced block (the first `{` after the signature to
/// its matching `}`).
///
/// Brace matching runs over the code-only view, so a `{` / `}` inside a string,
/// char, or comment is already blanked and can neither prematurely close a body
/// (hiding a later offense) nor fold a sibling in. The returned body text is itself
/// code-only — every later structural scan (the Decl classification, the raw close
/// site, the delegation) sees only real code, never a token buried in prose.
fn function_bodies(code: &str) -> Vec<(usize, String)> {
    let bytes = code.as_bytes();
    let mut bodies = Vec::new();
    // 1-based line number of each byte offset (precomputed prefix newline count).
    let line_of =
        |offset: usize| -> usize { code[..offset].bytes().filter(|&b| b == b'\n').count() + 1 };

    let mut search = 0usize;
    while let Some(rel) = code[search..].find("fn ") {
        let fn_at = search + rel;
        // Find the first `{` at/after the `fn` (the body open); bail if none.
        let Some(open_rel) = code[fn_at..].find('{') else {
            break;
        };
        let open = fn_at + open_rel;
        // Brace-match from `open` to its pair.
        let mut depth = 0i32;
        let mut end = open;
        for (idx, &b) in bytes[open..].iter().enumerate() {
            match b {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = open + idx + 1;
                        break;
                    }
                }
                _ => {}
            }
        }
        let start_line = line_of(fn_at);
        bodies.push((start_line, code[fn_at..end].to_string()));
        // Advance past this `fn` token (NOT past the body — nested/sibling `fn`s
        // inside or after are picked up on subsequent iterations).
        search = fn_at + 3;
    }
    bodies
}

/// The non-`Decl` provider-path-kind discriminants — the kinds a raw `close_dts`
/// may legitimately serve outside the owner (the generic non-Decl closers and the
/// `Api`/`Ide`/`Shadow` arms of the provider-state close dispatch).
const NON_DECL_KIND_TOKENS: &[&str] = &[
    "ProviderPathKind::Api",
    "ProviderPathKind::Ide",
    "ProviderPathKind::Shadow",
];

/// Scan a production source string for the "stray Decl close" footgun: a raw
/// provider `close_dts(` call that is REACHABLE from a `ProviderPathKind::Decl`
/// classification WITHOUT routing through the lifecycle owner's serialized
/// [`guarded_close`]. The owner is the sole authority that pairs `Decl` with a
/// `close_dts`; outside it the ONLY safe pairing is a `Decl` branch that DELEGATES
/// to `guarded_close` while its raw `close_dts` sits on a NON-`Decl` arm (e.g.
/// `provider_state::close_provider_paths`, whose `Api` arm closes directly and whose
/// `Decl` arm is `unreachable!`, delegated above).
///
/// Detection is PER CALL SITE, over the CODE-ONLY view (comments + strings blanked),
/// so the footgun is caught no matter how the tokens are separated and the word
/// `guarded_close` in a comment can never exempt a genuine stray close. For each raw
/// `close_dts(` call site the rule reads its governing discriminant — the nearest
/// `ProviderPathKind::…` token textually BEFORE the call within the same function:
///
///   * nearest governing kind is a NON-`Decl` kind (`Api`/`Ide`/`Shadow`) → the
///     close serves a non-Decl arm → SAFE (this is the `close_provider_paths` shape);
///   * nearest governing kind is `ProviderPathKind::Decl` → a raw close on a `Decl`
///     control path → OFFENSE (a preceding `guarded_close(` delegation does not
///     exempt it — see below);
///   * no governing kind at all but the function classifies `Decl` somewhere → the
///     close is not fenced to a non-Decl kind and is reachable from the `Decl` path
///     → OFFENSE.
///
/// The sanctioned delegation is the `guarded_close(` CALL ITSELF, which is not a raw
/// `close_dts(` and is therefore never an offense. It grants NO exemption to any
/// OTHER raw close: a `Decl`-reachable raw `close_dts(` is flagged regardless of a
/// preceding `guarded_close(` in the same region, so delegating and THEN raw-closing
/// (or delegating in one branch and straying in another) is still caught on the stray
/// close. Returns each offending site's 1-based line (with the offending close line's
/// text) for a precise failure message.
fn decl_close_offenses(src: &str) -> Vec<(usize, String)> {
    let code = code_only(src);
    let mut offenses = Vec::new();
    for (start_line, body) in function_bodies(&code) {
        let classifies_decl = body.contains("ProviderPathKind::Decl");
        // The byte offsets within `body` of every real `close_dts(` call site.
        let mut close_at = 0usize;
        while let Some(rel) = body[close_at..].find("close_dts(") {
            let site = close_at + rel;
            close_at = site + "close_dts(".len();

            // The governing discriminant: the nearest `ProviderPathKind::…` token
            // before this call site (within the function body, code-only).
            let prefix = &body[..site];
            let decl_pos = prefix.rfind("ProviderPathKind::Decl");
            let non_decl_pos = NON_DECL_KIND_TOKENS
                .iter()
                .filter_map(|tok| prefix.rfind(tok))
                .max();

            let is_offense = match decl_pos {
                // Governed by a Decl classification that is the MOST RECENT kind token
                // before the close (no non-Decl kind intervened): a raw Decl-reachable
                // close. ALWAYS an offense. A `guarded_close(` delegation earlier in
                // the region grants NO exemption — the sanctioned delegation IS the
                // `guarded_close(` call, which is itself not a raw `close_dts(`; once a
                // region has delegated, a SUBSEQUENT raw close in that same region is
                // exactly the stray-after-delegate footgun this guard forbids.
                Some(dp) if non_decl_pos.is_none_or(|ndp| dp > ndp) => true,
                // A Decl token exists before the close but a non-Decl kind is more
                // recent → the close serves that non-Decl arm → safe.
                Some(_) => false,
                // No Decl token before the close at all. Safe unless the function
                // classifies Decl somewhere — then this close is not fenced to a
                // non-Decl kind and is reachable from the Decl path.
                None => classifies_decl,
            };

            if is_offense {
                let line_in_body = body[..site].bytes().filter(|&b| b == b'\n').count();
                let close_line_text = body
                    .lines()
                    .nth(line_in_body)
                    .map(|l| l.trim().to_string())
                    .unwrap_or_default();
                offenses.push((start_line + line_in_body, close_line_text));
            }
        }
    }
    offenses
}

#[test]
fn no_stray_declaration_overlay_close_outside_the_lifecycle_owner() {
    let root = src_root();
    let mut files = Vec::new();
    collect_production_rs_files(&root, &root, &mut files);
    assert!(
        !files.is_empty(),
        "guard found no LSP source files to scan under {root:?}"
    );

    let mut offenders: Vec<String> = Vec::new();
    for (rel, path) in &files {
        let src = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        let production = production_only(&src);
        for (lineno, text) in decl_close_offenses(&production) {
            offenders.push(format!("{rel}:{lineno}: {text}"));
        }
    }

    assert!(
        offenders.is_empty(),
        "a declaration-overlay close (`close_dts` paired with `ProviderPathKind::Decl`) \
         exists OUTSIDE the lifecycle owner ({OWNER_MODULE_BASENAME}). Every `Decl` \
         overlay close MUST route through `DeclOverlayOwner::guarded_close` (the sole \
         serialized authority); a raw stray close races a concurrent overlay open and \
         strands an open carrier root on TS2307. Offending sites:\n  {}",
        offenders.join("\n  ")
    );
}

// ---------------------------------------------------------------------------
// Detector self-tests: prove the offense rule DISCRIMINATES — it flags a raw
// `close_dts` that is Decl-reachable however the tokens are separated (a real
// `guarded_close(` delegation in the same region, a delegation in a sibling
// branch, a multi-line comment, a `}` inside a string, a `&'static`/`<'a>`
// lifetime, a raw string carrying embedded quotes/braces, a nested block comment)
// and does NOT flag the one safe shape (a `Decl` branch that DELEGATES via
// `guarded_close` while its raw `close_dts` sits on a non-`Decl` arm) nor tokens
// that live only in prose/strings. These guard the GUARD itself against
// false-negative regressions: a `guarded_close(` delegation wrongly exempting a
// SUBSEQUENT raw close, a brace matcher fooled by string/comment braces, a
// lifetime apostrophe blanking real code as if it opened a string, and a raw
// string or nested comment desyncing the code-only scan.
// ---------------------------------------------------------------------------

/// The legitimate delegation shape (`provider_state::close_provider_paths`): the
/// `Decl` arm DELEGATES through `guarded_close`, and the only raw `close_dts` is on
/// the non-`Decl` `Api` arm. MUST NOT be flagged.
#[test]
fn safe_decl_delegation_with_non_decl_raw_close_is_not_flagged() {
    let src = r#"
        async fn close_provider_paths(&self, paths: &[(ProviderPathKind, String)]) {
            for (kind, path) in paths {
                if *kind == ProviderPathKind::Decl {
                    let target = self.decl_overlay_owner.close_target_for(path);
                    self.decl_overlay_owner
                        .guarded_close(sync, &self.provider_sync_states, &[target])
                        .await;
                    continue;
                }
                let result = match kind {
                    ProviderPathKind::Ide => sync.close_tsx(path).await,
                    ProviderPathKind::Api => sync.close_dts(path).await,
                    ProviderPathKind::Shadow => sync.close_file(path).await,
                    ProviderPathKind::Decl => unreachable!("Decl is delegated above"),
                };
                let _ = result;
            }
        }
    "#;
    assert!(
        decl_close_offenses(src).is_empty(),
        "the safe delegation (Decl routed through guarded_close, raw close_dts on the \
         Api arm) must NOT be flagged; got {:?}",
        decl_close_offenses(src)
    );
}

/// The footgun: a `Decl`-guarded raw `close_dts` that does NOT delegate, with a
/// MULTI-LINE comment (mentioning `guarded_close`) between the `Decl` token and the
/// raw close — the exact shape a per-function `guarded_close` substring escape and a
/// fixed line window would BOTH miss. MUST be flagged.
#[test]
fn decl_guarded_raw_close_with_intervening_comment_is_flagged() {
    let src = r#"
        async fn stray(&self, kind: &ProviderPathKind, path: &str) {
            if *kind == ProviderPathKind::Decl {
                // This block does not actually delegate. It only mentions the word
                // guarded_close in prose, across several lines, to try to evade a
                // substring escape. The real owner delegation is `guarded_close(`,
                // an actual call — not a word in a comment. A fixed N-line window
                // between the Decl token and the close below would also miss it.
            } else if let Err(_e) = sync.close_dts(path).await {
            }
        }
    "#;
    let offenses = decl_close_offenses(src);
    assert!(
        !offenses.is_empty(),
        "a Decl-guarded raw close_dts that does NOT delegate must be flagged even when a \
         multi-line comment mentioning `guarded_close` separates the tokens; got no offenses"
    );
}

/// Delegate-AND-stray: the function delegates the `Decl` path via `guarded_close`
/// in one branch BUT ALSO performs a stray raw `Decl`-guarded `close_dts` in a
/// SECOND branch. The presence of a real `guarded_close` call must NOT exempt the
/// stray close — the per-function substring escape was exactly this hole.
#[test]
fn function_that_delegates_once_and_also_strays_is_flagged() {
    let src = r#"
        async fn mixed(&self, kind: &ProviderPathKind, path: &str, other: &str) {
            if *kind == ProviderPathKind::Decl {
                self.decl_overlay_owner.guarded_close(sync, states, &[t]).await;
                return;
            }
            if *kind == ProviderPathKind::Decl {
                // a stray second handling of Decl that closes directly
                let _ = sync.close_dts(other).await;
            }
        }
    "#;
    assert!(
        !decl_close_offenses(src).is_empty(),
        "a function that delegates Decl once via guarded_close AND ALSO performs a stray \
         Decl-reachable raw close_dts must be flagged (the substring escape must not exempt \
         the whole function); got no offenses"
    );
}

/// Brace-matcher robustness: a `}` INSIDE a string literal must not prematurely end
/// a function body and let a later in-body offense escape the scan. The offending
/// `Decl`-guarded close sits AFTER a string containing a `}`.
#[test]
fn brace_inside_string_does_not_under_extend_body_and_hide_offense() {
    let src = "
        async fn tricky(&self, kind: &ProviderPathKind, path: &str) {
            let _label = \"a closing brace } inside a string\";
            if *kind == ProviderPathKind::Decl {
                let _ = sync.close_dts(path).await;
            }
        }
    ";
    assert!(
        !decl_close_offenses(src).is_empty(),
        "a `}}` inside a string literal must not under-extend the function body and hide a \
         later Decl-reachable raw close_dts; got no offenses"
    );
}

/// A function that mentions `ProviderPathKind::Decl` and `close_dts` only inside a
/// COMMENT / string (no real code-level pairing) must NOT be flagged — stripping
/// comment + string content keeps the detector from false-positiving on prose.
#[test]
fn decl_and_close_only_inside_comment_or_string_is_not_flagged() {
    let src = r#"
        async fn only_prose(&self, path: &str) {
            // Historical note: ProviderPathKind::Decl used to call close_dts(path)
            // directly here; it now routes through the owner. This is prose only.
            let _doc = "ProviderPathKind::Decl + close_dts( in a string, not code";
            let _ = path;
        }
    "#;
    assert!(
        decl_close_offenses(src).is_empty(),
        "a Decl/close_dts pairing that exists ONLY inside comments/strings must NOT be \
         flagged; got {:?}",
        decl_close_offenses(src)
    );
}

/// A raw `close_dts` on a non-`Decl` arm in a function that NEVER classifies `Decl`
/// is fine (the generic non-Decl closers). MUST NOT be flagged.
#[test]
fn non_decl_close_without_any_decl_classification_is_not_flagged() {
    let src = r#"
        async fn close_api(&self, kind: &NonDeclProviderPathKind, path: &str) {
            match kind {
                NonDeclProviderPathKind::Api => sync.close_dts(path).await,
                NonDeclProviderPathKind::Ide => sync.close_tsx(path).await,
            };
        }
    "#;
    assert!(
        decl_close_offenses(src).is_empty(),
        "a non-Decl close in a function that never classifies ProviderPathKind::Decl must \
         NOT be flagged; got {:?}",
        decl_close_offenses(src)
    );
}

/// Delegate-THEN-stray in the SAME `Decl` region: the `Decl` arm DELEGATES through
/// `guarded_close` and THEN — in the same arm, after the delegation — also issues a
/// raw `close_dts`. A preceding `guarded_close(` delegation must NOT exempt a
/// SUBSEQUENT raw `Decl`-reachable close: the sanctioned `guarded_close(` call is the
/// only thing that is not itself a raw close, and it grants no blanket exemption to a
/// later raw close in the same region. The stray raw close MUST be flagged.
#[test]
fn delegate_then_stray_raw_close_in_same_region_is_flagged() {
    let src = r#"
        async fn delegate_then_stray(&self, kind: &ProviderPathKind, path: &str) {
            if *kind == ProviderPathKind::Decl {
                self.decl_overlay_owner.guarded_close(sync, states, &[t]).await;
                sync.close_dts(path).await;
            }
        }
    "#;
    assert!(
        !decl_close_offenses(src).is_empty(),
        "a raw close_dts that follows a guarded_close delegation IN THE SAME Decl region \
         must be flagged — the delegation grants no exemption to a subsequent raw close; \
         got no offenses"
    );
}

/// Positive control for the delegate-then-stray rule: a `Decl` arm that ONLY
/// delegates (a single `guarded_close(` call and no subsequent raw `close_dts`) is
/// the sanctioned shape and MUST NOT be flagged. This pins the rule to "a raw close
/// after the delegation" rather than "any function containing a delegation".
#[test]
fn pure_delegation_with_no_subsequent_raw_close_is_not_flagged() {
    let src = r#"
        async fn pure_delegation(&self, kind: &ProviderPathKind, path: &str) {
            if *kind == ProviderPathKind::Decl {
                let target = self.decl_overlay_owner.close_target_for(path);
                self.decl_overlay_owner
                    .guarded_close(sync, &self.provider_sync_states, &[target])
                    .await;
            }
        }
    "#;
    assert!(
        decl_close_offenses(src).is_empty(),
        "a Decl arm that only delegates via guarded_close (no subsequent raw close_dts) \
         must NOT be flagged; got {:?}",
        decl_close_offenses(src)
    );
}

/// Lifetime soundness: a `Decl`-governed raw `close_dts` that sits AFTER a
/// `&'static` lifetime in the SAME function MUST be flagged. A `'` that begins a
/// lifetime is ordinary code — it must NOT open a char/string span (which would
/// blank from the apostrophe to the next `'`/EOF, swallowing the `Decl` token and
/// the close and hiding the offense).
#[test]
fn decl_close_after_static_lifetime_is_flagged() {
    let src = r#"
        async fn after_lifetime(&self, kind: &ProviderPathKind, label: &'static str) {
            if *kind == ProviderPathKind::Decl {
                let _ = sync.close_dts(label).await;
            }
        }
    "#;
    assert!(
        !decl_close_offenses(src).is_empty(),
        "a Decl-reachable raw close_dts after a `&'static` lifetime must be flagged — a \
         lifetime apostrophe must not open a string span that blanks the close; got no \
         offenses"
    );
}

/// Lifetime soundness: a generic lifetime parameter `<'a>` (and a `&'a` reference)
/// is ordinary code and must NOT blank the `Decl`-governed raw `close_dts` that
/// follows it. MUST be flagged.
#[test]
fn decl_close_after_generic_lifetime_param_is_flagged() {
    let src = r#"
        async fn after_generic_lifetime<'a>(&self, kind: &ProviderPathKind, path: &str) {
            if *kind == ProviderPathKind::Decl {
                let _ = sync.close_dts(path).await;
            }
        }
    "#;
    assert!(
        !decl_close_offenses(src).is_empty(),
        "a Decl-reachable raw close_dts after a `<'a>` generic lifetime must be flagged — \
         lifetime apostrophes must not open a string span; got no offenses"
    );
}

/// Char-literal control (the counterpart to the lifetime soundness fix): genuine
/// char literals — including one whose single character is a brace (`'}'`) and an
/// escaped char (`'\n'`) — must STILL be blanked. If `'}'` were mis-classified as a
/// lifetime its `}` would under-extend the body and hide the offense; if it is
/// correctly blanked the `Decl`-governed close that follows is still flagged.
#[test]
fn genuine_char_literals_are_blanked_and_offense_still_flagged() {
    let src = r#"
        async fn char_control(&self, kind: &ProviderPathKind, path: &str) {
            let _brace = '}';
            let _nl = '\n';
            if *kind == ProviderPathKind::Decl {
                let _ = sync.close_dts(path).await;
            }
        }
    "#;
    assert!(
        !decl_close_offenses(src).is_empty(),
        "genuine char literals (`'}}'`, `'\\n'`) must be blanked so a brace inside a char \
         literal does not under-extend the body; the Decl-reachable close must still be \
         flagged; got no offenses"
    );
}

/// Raw-string + nested-block-comment soundness: a `Decl`-governed raw `close_dts`
/// that follows a raw string `r#"…")"#` (carrying embedded `}`, `)`, and `"`) AND a
/// NESTED block comment `/* /* */ */` MUST be flagged. The raw string must be
/// consumed by its `"#` close (an embedded `"` must not split it and desync the
/// scan into blanking the close), and the nested comment must track depth so its
/// inner `*/` does not end it early and leave the close visible-but-mangled.
#[test]
fn decl_close_after_raw_string_and_nested_comment_is_flagged() {
    let src = r##"
        async fn after_raw_and_nested(&self, kind: &ProviderPathKind, path: &str) {
            let _r = r#"a raw ) string with } and " inside"#;
            /* outer /* inner */ still comment */
            if *kind == ProviderPathKind::Decl {
                let _ = sync.close_dts(path).await;
            }
        }
    "##;
    assert!(
        !decl_close_offenses(src).is_empty(),
        "a Decl-reachable raw close_dts after a raw string (with embedded braces/quotes) \
         and a nested block comment must be flagged — the raw string and nested comment \
         must not desync the scan and hide the close; got no offenses"
    );
}

/// Raw-string control: a raw string that genuinely CONTAINS the offense tokens
/// (`ProviderPathKind::Decl` and `close_dts(`) as TEXT must NOT be flagged — a raw
/// string is blanked like any other string, so tokens inside it are not code.
#[test]
fn tokens_inside_a_raw_string_are_not_flagged() {
    let src = r##"
        async fn raw_string_prose(&self, path: &str) {
            let _doc = r#"ProviderPathKind::Decl once called close_dts( here, as text"#;
            let _ = path;
        }
    "##;
    assert!(
        decl_close_offenses(src).is_empty(),
        "Decl/close_dts tokens that appear only inside a raw string must NOT be flagged; \
         got {:?}",
        decl_close_offenses(src)
    );
}
