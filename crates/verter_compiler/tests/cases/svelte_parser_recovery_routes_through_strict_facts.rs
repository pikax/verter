//! ARCHITECTURE GUARD: every Svelte-parser RECOVERY primitive routes through a
//! strict-fact decision — scoped to the PRIMITIVE SITE, not the whole function.
//!
//! Verter's Svelte tokenizer is intentionally infallible / recovery-based — it never
//! panics and always emits a faithful tree. Each recovery point official's STRICT parser
//! rejects MUST push a typed `SvelteStrictParseError` (via a `record_*` strict-fact
//! helper) so the official-reject gate fails closed instead of emitting a divergent
//! `Main`. Hand-auditing those recovery points has repeatedly missed leaks, so this
//! guard mechanically reduces the future audit surface: every RECOVERY PRIMITIVE in the
//! parser source — an EOF jump (`self.pos = self.len()`) or a raw SCAN-TO-`>` loop
//! (`while … self.at(…) != b'>'`) — must have a strict-fact decision (a real strict-fact
//! `record_*` call / a `strict_parse_errors.push`) in the SAME enclosing block (branch)
//! as the primitive, OR sit in a function on the EXPLICIT allowlist of recovery helpers
//! whose CALLER owns the fact.
//!
//! The scoping is the point: a FUNCTION-scoped check passes vacuously when a NEW
//! unblessed recovery primitive is added to a branch of an already-recording function
//! (a different branch's `record_*` blesses the whole function). This guard instead
//! associates each primitive with the innermost `{ … }` block that contains it and
//! requires a strict fact WITHIN that block — so a new unrecorded branch fails even when
//! a sibling branch records. The strict-fact match is the REAL helper family only (the
//! `record_*` calls that push a `SvelteStrictParseError`, plus `strict_parse_errors.push`)
//! — NOT any `record_` substring (a `record_close_violation` / `record_stray_or_void_close`
//! pushes a `CloseTagViolation`, a DIFFERENT rail, and must NOT bless a strict-fact-less
//! recovery).
//!
//! The scanned files are the two parser source files only (`tokenizer.rs` +
//! `tokenizer_scan.rs`). The guard is discriminating: [`detects_unblessed_eof_jump`] and
//! [`detects_unblessed_scan_to_gt`] prove it fires on a planted recovery primitive in a
//! function with no strict-fact call; [`detects_unblessed_recovery_in_sibling_branch`]
//! proves the PRIMITIVE-SITE scoping (a branch with an unrelated `record_close_violation`
//! does not bless a DIFFERENT branch's unblessed recovery — the bug a function-scoped
//! check missed); [`accepts_blessed_recovery`] / [`accepts_allowlisted_function`] prove
//! it does not fire on a recovery that records a fact in its block or on an allowlisted
//! helper. All discrimination is exercised on INLINE-STRING fixtures, never by editing
//! production `src/`.

use std::path::PathBuf;

/// The parser source files this guard governs.
fn parser_source_files() -> Vec<PathBuf> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/svelte/parser");
    vec![root.join("tokenizer.rs"), root.join("tokenizer_scan.rs")]
}

/// Functions ALLOWED to contain a recovery primitive WITHOUT minting a strict fact in
/// the primitive's block — each because its CALLER owns the fact decision, or because it
/// is a pure non-recovering scanner whose result the caller classifies. Adding a name
/// here is a CONSCIOUS decision (the reviewer asserts the caller records the fact); the
/// default for a new recovery point is to record a fact, not to be allowlisted.
///
/// Only functions that ACTUALLY contain a recovery primitive belong here (a no-primitive
/// function never reaches the allowlist check, so a dead entry is meaningless) — the
/// `allowlist_entries_are_live` test enforces that this list carries no dead rows.
const RECOVERY_HELPER_ALLOWLIST: &[&str] = &[
    // `consume_close_tag` scans to `>` to CONSUME a close tag; its caller
    // (`consume_and_classify_close`) classifies the boundary and records the fact.
    "consume_close_tag",
    // `find_close_tag` scans raw content to LOCATE a `</tag>` close (the TOP-LEVEL
    // `<script>` / `<style>` raw blocks); a miss is reported as `None` and the CALLER
    // records the fact. (The NESTED raw close `find_nested_raw_close` records its OWN
    // strict fact in-block, so it is not allowlisted.)
    "find_close_tag",
];

/// The REAL strict-fact tokens — a call that pushes a `SvelteStrictParseError` onto the
/// `strict_parse_errors` stream. The `record_*` strict-fact helpers (defined in
/// `strict_facts.rs`) plus the direct push and the shared private sink. Deliberately
/// EXCLUDES `record_close_violation` / `record_stray_or_void_close` (those push a
/// `CloseTagViolation`, a separate rail) so a close-violation-only recovery is NOT
/// blessed as a strict-fact recovery.
const STRICT_FACT_TOKENS: &[&str] = &[
    "record_strict_parse_error",
    "record_tag_invalid_name",
    "record_expected_token",
    "record_empty_attribute_value",
    "record_nameless_close",
    "record_element_unclosed",
    "record_unexpected_eof",
    "record_css_expected_identifier",
    "strict_parse_errors.push",
];

/// A byte-aligned "brace skeleton" of a Rust source: every byte INSIDE a `//` / `/* */`
/// comment, a `"…"` string literal, or a `'…'` / `b'…'` CHAR literal is replaced by a
/// space (so a `b'{'` / `b'}'` / `b'>'` char literal can never be miscounted as a block
/// brace), with length + newlines preserved so byte offsets stay aligned with the
/// original. Used ONLY for brace matching (`functions` / `innermost_block_range`); the
/// recovery-primitive scan deliberately reads the original (comment-stripped) bytes so a
/// genuine `b'>'` close-scan literal is still seen.
fn strip_to_brace_skeleton(src: &str) -> String {
    let bytes = src.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    let blank = |out: &mut Vec<u8>, b: u8| out.push(if b == b'\n' { b'\n' } else { b' ' });
    while i < bytes.len() {
        let b = bytes[i];
        // Line comment.
        if b == b'/' && bytes.get(i + 1) == Some(&b'/') {
            while i < bytes.len() && bytes[i] != b'\n' {
                blank(&mut out, bytes[i]);
                i += 1;
            }
            continue;
        }
        // Block comment.
        if b == b'/' && bytes.get(i + 1) == Some(&b'*') {
            while i < bytes.len() && !(bytes[i] == b'*' && bytes.get(i + 1) == Some(&b'/')) {
                blank(&mut out, bytes[i]);
                i += 1;
            }
            // Blank the closing `*/`.
            for _ in 0..2 {
                if i < bytes.len() {
                    blank(&mut out, bytes[i]);
                    i += 1;
                }
            }
            continue;
        }
        // String literal.
        if b == b'"' {
            out.push(b' '); // the opening quote
            i += 1;
            while i < bytes.len() && bytes[i] != b'"' {
                if bytes[i] == b'\\' && i + 1 < bytes.len() {
                    blank(&mut out, bytes[i]);
                    blank(&mut out, bytes[i + 1]);
                    i += 2;
                    continue;
                }
                blank(&mut out, bytes[i]);
                i += 1;
            }
            if i < bytes.len() {
                out.push(b' ');
                i += 1;
            }
            continue;
        }
        // Char / byte-char literal: `'x'` or `b'x'` (the `b` prefix is an ordinary ident
        // byte already emitted). Only treat `'` as a char-literal open when it is NOT a
        // lifetime (`'a` followed by a non-`'` ident) — the parser source has no lifetimes
        // inside method bodies, so a simple "`'` opens a char literal" rule is safe here.
        if b == b'\'' {
            out.push(b' '); // opening quote
            i += 1;
            if i < bytes.len() && bytes[i] == b'\\' && i + 1 < bytes.len() {
                blank(&mut out, bytes[i]);
                blank(&mut out, bytes[i + 1]);
                i += 2;
            } else if i < bytes.len() {
                blank(&mut out, bytes[i]);
                i += 1;
            }
            // closing quote (if present)
            if i < bytes.len() && bytes[i] == b'\'' {
                out.push(b' ');
                i += 1;
            }
            continue;
        }
        out.push(b);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| src.to_string())
}

/// Extract the top-level `fn <name>(` function blocks from a Rust source, returning
/// `(name, body)` pairs (the body INCLUDING its outer braces, from the ORIGINAL source).
/// Brace matching runs over the [`strip_to_brace_skeleton`] so a `b'{'` / `b'}'` char
/// literal is never miscounted — but the returned body is the original text (so the
/// downstream primitive scan sees the real `b'>'` literals).
fn functions(src: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let skeleton = strip_to_brace_skeleton(src);
    let sk = skeleton.as_bytes();
    let mut i = 0;
    while i < src.len() {
        let Some(rel) = src[i..].find("fn ") else {
            break;
        };
        let fn_pos = i + rel;
        // Require `fn ` to start a word (the previous non-space char is not alphanumeric),
        // so `transform_fn` / `effect_fn` substrings do not match.
        let prev = src[..fn_pos]
            .trim_end_matches(' ')
            .chars()
            .last()
            .unwrap_or(' ');
        if prev.is_alphanumeric() || prev == '_' {
            i = fn_pos + 3;
            continue;
        }
        // Parse the name (identifier after `fn `).
        let after = &src[fn_pos + 3..];
        let name_end = after
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .unwrap_or(after.len());
        let name = after[..name_end].to_string();
        // Find the body open brace `{` after the signature (in the SKELETON, so a `{` in a
        // doc comment / string does not count), then brace-match the body.
        let Some(brace_rel) = skeleton[fn_pos..].find('{') else {
            break;
        };
        let body_start = fn_pos + brace_rel;
        let mut depth = 0usize;
        let mut j = body_start;
        let mut end = body_start;
        while j < sk.len() {
            match sk[j] {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = j + 1;
                        break;
                    }
                }
                _ => {}
            }
            j += 1;
        }
        if name_end > 0 {
            out.push((name, src[body_start..end].to_string()));
        }
        i = end.max(fn_pos + 3);
    }
    out
}

/// Reconstruct source with comment-only lines dropped (so a "don't do this" example or a
/// doc comment never trips the scanner), preserving line structure + byte offsets within
/// each retained line (blanked lines keep their newline so byte ranges stay aligned).
fn strip_comment_lines(src: &str) -> String {
    src.lines()
        .map(|line| {
            let t = line.trim_start();
            if t.starts_with("//") {
                // Blank the line but keep its length so byte offsets are preserved.
                " ".repeat(line.len())
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The byte positions of every RECOVERY PRIMITIVE occurrence in `code` (comment-stripped):
/// an EOF jump (`self.pos = self.len()`) or the start of a scan-to-`>` `while` loop. Each
/// occurrence is located independently so the per-site (per-block) scoping can classify it.
fn recovery_primitive_sites(code: &str) -> Vec<usize> {
    let mut sites = Vec::new();
    // EOF jumps — match both spacing forms.
    for needle in ["self.pos = self.len()", "self.pos=self.len()"] {
        let mut from = 0;
        while let Some(rel) = code[from..].find(needle) {
            sites.push(from + rel);
            from += rel + needle.len();
        }
    }
    // Scan-to-`>` loops: a `while` whose condition tests a byte against `b'>'`. Located by
    // the `b'>'` close-byte literal that is part of a `while … !=` condition; we anchor on
    // the nearest preceding `while` on the same logical span.
    let mut from = 0;
    while let Some(rel) = code[from..].find("!= b'>'") {
        let pos = from + rel;
        // Confirm a `while` precedes this `!= b'>'` reasonably close (same statement) so a
        // bare `self.at(p) != b'>'` boolean classifier expression is still treated as a
        // recovery primitive (it IS one — `close_tag_trailing_token` is allowlisted).
        let window_start = pos.saturating_sub(120);
        let preceding = &code[window_start..pos];
        if preceding.contains("while") || preceding.contains("&&") || preceding.contains("p <") {
            sites.push(pos);
        } else {
            // A lone `!= b'>'` with no scan context — still a `>`-boundary test; treat it
            // as a primitive site (conservative: better to require a fact than to miss).
            sites.push(pos);
        }
        from = pos + "!= b'>'".len();
    }
    sites
}

/// The innermost `{ … }` block byte-range `[open, close]` (inclusive of both braces) that
/// encloses `pos` within `body` (which itself starts and ends with the function's outer
/// braces). Falls back to the whole `body` when `pos` is at the function-body top level.
/// Brace matching runs over the [`strip_to_brace_skeleton`] of `body` so a `b'{'` / `b'}'`
/// char literal is never miscounted.
fn innermost_block_range(body: &str, pos: usize) -> (usize, usize) {
    let skeleton = strip_to_brace_skeleton(body);
    let bytes = skeleton.as_bytes();
    // Walk forward to `pos`, maintaining a stack of open-brace positions; the top of the
    // stack at `pos` is the innermost enclosing block's open brace.
    let mut stack: Vec<usize> = Vec::new();
    let mut p = 0;
    while p < pos && p < bytes.len() {
        match bytes[p] {
            b'{' => stack.push(p),
            b'}' => {
                stack.pop();
            }
            _ => {}
        }
        p += 1;
    }
    let open = match stack.last() {
        Some(&o) => o,
        None => return (0, body.len()),
    };
    // Brace-match forward from `open` to its `}`.
    let mut depth = 0usize;
    let mut q = open;
    while q < bytes.len() {
        match bytes[q] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return (open, q + 1);
                }
            }
            _ => {}
        }
        q += 1;
    }
    (open, body.len())
}

/// The "already-recorded fact" guard idioms — a block that branches on the strict-fact
/// COUNT (`facts_before` / `strict_parse_errors.len()`) only mints its fallback fact when
/// NO earlier fact was recorded, so a strict fact DOMINATES the recovery exit on every
/// path (either the fallback records, or an earlier fact already exists). Recognising this
/// idiom keeps the dominance check from false-flagging the legit `*_or_recover` fallback
/// sites while still rejecting a genuinely-conditional record (the leak shape).
const FACT_COUNT_GUARD_TOKENS: &[&str] = &["facts_before", "strict_parse_errors.len()"];

/// Whether a strict-fact `record_*` call DOMINATES the recovery primitive at
/// `primitive_rel` within `block_src` (the innermost enclosing block, starting with its
/// own `{`). Dominance proxy — a fact is on EVERY path reaching the recovery exit when:
///   (a) a strict-fact token occurs at BRACE-DEPTH 0 of the block (directly in the block,
///       NOT buried inside a deeper nested `if {...}` / match-arm the primitive sits
///       outside of) textually BEFORE the primitive — an UNCONDITIONAL in-block record; OR
///   (b) the block uses the "already-recorded fact-count" guard idiom (the fallback fact is
///       gated on `strict_parse_errors.len() == facts_before`, so an earlier fact is
///       guaranteed on the alternate path).
/// A record buried in a deeper conditional (or a sibling branch) with NEITHER property does
/// NOT dominate — that is the leak shape this catches.
fn strict_fact_dominates(block_src: &str, primitive_rel: usize) -> bool {
    // (b) The fact-count guard idiom anywhere in the block proves a dominating prior fact.
    if FACT_COUNT_GUARD_TOKENS
        .iter()
        .any(|t| block_src.contains(t))
    {
        return true;
    }
    // (a) A strict-fact token at relative brace-depth 0, textually before the primitive.
    // Depth is tracked over the brace skeleton so a `b'{'` char literal is never miscounted.
    let skeleton = strip_to_brace_skeleton(block_src);
    let sk = skeleton.as_bytes();
    for token in STRICT_FACT_TOKENS {
        let mut from = 0;
        while let Some(rel) = block_src[from..].find(token) {
            let pos = from + rel;
            from = pos + token.len();
            if pos >= primitive_rel {
                continue;
            }
            // Relative depth at `pos`: the block source starts with the block's own `{`
            // (depth 1 after it), so the block body sits at running-depth 1 ⇒ relative
            // depth 0. A token directly in the block has running depth exactly 1.
            let mut depth = 0i32;
            for &b in &sk[..pos] {
                match b {
                    b'{' => depth += 1,
                    b'}' => depth -= 1,
                    _ => {}
                }
            }
            if depth == 1 {
                return true;
            }
        }
    }
    false
}

/// The core analysis: every recovery-primitive SITE must have a strict-fact decision that
/// DOMINATES the recovery exit (a strict fact on every path reaching the primitive) in its
/// innermost enclosing block, OR its function is allowlisted. Returns the offending
/// `fn-name` for each unblessed site (a function may appear once per unblessed site).
fn unblessed_recovery_functions(src: &str) -> Vec<String> {
    let mut offenders = Vec::new();
    for (name, body) in functions(src) {
        let code = strip_comment_lines(&body);
        let sites = recovery_primitive_sites(&code);
        if sites.is_empty() {
            continue;
        }
        if RECOVERY_HELPER_ALLOWLIST.contains(&name.as_str()) {
            continue;
        }
        for site in sites {
            let (open, close) = innermost_block_range(&code, site);
            let block_src = &code[open..close.min(code.len())];
            // The primitive's offset RELATIVE to the block (the block starts at `open`).
            let primitive_rel = site - open;
            if !strict_fact_dominates(block_src, primitive_rel) {
                offenders.push(name.clone());
            }
        }
    }
    offenders
}

/// The set of allowlist entries that ACTUALLY contain a recovery primitive in the live
/// parser source — a dead allowlist entry (a function with no primitive) is meaningless
/// theater and is pruned.
fn live_allowlist_entries(srcs: &[String]) -> Vec<String> {
    let mut live = Vec::new();
    for src in srcs {
        for (name, body) in functions(src) {
            let code = strip_comment_lines(&body);
            if !recovery_primitive_sites(&code).is_empty()
                && RECOVERY_HELPER_ALLOWLIST.contains(&name.as_str())
                && !live.contains(&name)
            {
                live.push(name);
            }
        }
    }
    live
}

#[test]
fn every_parser_recovery_primitive_records_a_strict_fact_or_is_allowlisted() {
    let mut violations = Vec::new();
    for path in parser_source_files() {
        let src = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        for name in unblessed_recovery_functions(&src) {
            violations.push(format!("{}::{name}", path.display()));
        }
    }
    assert!(
        violations.is_empty(),
        "these parser recovery-primitive SITES (an EOF jump or a scan-to-`>` loop) have NO \
         strict-parse fact (`record_*` / `strict_parse_errors.push`) in their enclosing \
         block and are NOT in an allowlisted helper — every recovery point official rejects \
         must push a `SvelteStrictParseError` so the official-reject gate fails closed:\n{}\
         \n\nIf the caller owns the fact, add the function to RECOVERY_HELPER_ALLOWLIST with \
         that rationale.",
        violations.join("\n")
    );
}

#[test]
fn allowlist_entries_are_live() {
    // Every allowlist entry must correspond to a function that ACTUALLY contains a
    // recovery primitive in the live parser source — a dead entry is removed (the
    // allowlist must reflect the real helper set, not stale names).
    let srcs: Vec<String> = parser_source_files()
        .into_iter()
        .map(|p| {
            std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
        })
        .collect();
    let live = live_allowlist_entries(&srcs);
    let dead: Vec<&str> = RECOVERY_HELPER_ALLOWLIST
        .iter()
        .copied()
        .filter(|name| !live.iter().any(|l| l == name))
        .collect();
    assert!(
        dead.is_empty(),
        "these RECOVERY_HELPER_ALLOWLIST entries are DEAD (the function has no recovery \
         primitive in the live parser source) — remove them so the allowlist reflects the \
         real helper set:\n{}",
        dead.join("\n")
    );
}

// ── Discrimination: the guard FIRES on a planted unblessed recovery (inline fixtures,
//    never by editing production src), and does NOT fire on a blessed / allowlisted one.

#[test]
fn detects_unblessed_eof_jump() {
    let fixture = r#"
        fn parse_something_new(&mut self) {
            // recovers by jumping to EOF, but records NO strict fact
            self.pos = self.len();
        }
    "#;
    assert_eq!(
        unblessed_recovery_functions(fixture),
        vec!["parse_something_new".to_string()],
        "a function with an EOF jump and no strict fact must be flagged"
    );
}

#[test]
fn detects_unblessed_scan_to_gt() {
    let fixture = r#"
        fn scan_something_new(&mut self) {
            let mut p = self.pos;
            while p < self.len() && self.at(p) != b'>' {
                p += 1;
            }
            self.pos = p;
        }
    "#;
    assert_eq!(
        unblessed_recovery_functions(fixture),
        vec!["scan_something_new".to_string()],
        "a function with a scan-to-`>` loop and no strict fact must be flagged"
    );
}

#[test]
fn detects_unblessed_recovery_in_sibling_branch() {
    // The PRIMITIVE-SITE scoping property: branch A records an UNRELATED close-violation
    // fact (`record_close_violation`, a DIFFERENT rail), branch B has an unblessed EOF
    // jump. A FUNCTION-scoped check (any `record_` substring) would wrongly bless the
    // whole function; the block-scoped guard MUST still flag branch B.
    let fixture = r#"
        fn parse_two_branches(&mut self) {
            if self.cur() == b'/' {
                // branch A: records a CLOSE-TAG violation (not a strict fact)
                self.record_close_violation(CloseTagViolationKind::Unclosed, name, span);
            } else {
                // branch B: an UNBLESSED EOF jump (no strict fact in this block)
                self.pos = self.len();
            }
        }
    "#;
    assert_eq!(
        unblessed_recovery_functions(fixture),
        vec!["parse_two_branches".to_string()],
        "a sibling branch's unrelated `record_close_violation` must NOT bless a DIFFERENT \
         branch's unblessed recovery (the primitive-site scoping)"
    );
}

#[test]
fn detects_nondominating_conditional_strict_fact_over_scan_recovery() {
    // The DOMINANCE property (the leak shape the old `find_nested_raw_close` had): a
    // scan-to-`>` recovery loop sits at the block's top level, but the strict-fact
    // `record_expected_token` is buried in a DEEPER conditional (`if trailing { ... }`) that
    // does NOT execute on every path reaching the recovery exit (the no-trailing-token /
    // EOF path records nothing). A block-CONTAINS-token check passes vacuously; the
    // dominance check MUST still flag it (the strict fact does not DOMINATE the recovery
    // return).
    let fixture = r#"
        fn scan_raw_close_leaky(&mut self) -> Option<usize> {
            let mut p = self.pos;
            while p < self.len() && self.at(p) != b'>' {
                p += 1;
            }
            if self.trailing_token(p) {
                self.record_expected_token(span);
            }
            Some(p)
        }
    "#;
    assert_eq!(
        unblessed_recovery_functions(fixture),
        vec!["scan_raw_close_leaky".to_string()],
        "a scan-to-`>` recovery whose only strict fact is buried in a DEEPER conditional \
         (not on every path to the recovery exit) must be flagged — the dominance property"
    );
}

#[test]
fn accepts_fact_count_guard_fallback_recovery() {
    // The legit `*_or_recover` fallback idiom: the fallback strict fact is gated on the
    // strict-fact COUNT (`strict_parse_errors.len() == facts_before`), so a fact DOMINATES
    // the EOF recovery exit on EVERY path (either the fallback records, or an earlier fact
    // already exists). The dominance check must NOT flag this (it is the production
    // `parse_open_tag_attributes_or_recover` / `parse_element_or_recover` shape).
    let fixture = r#"
        fn parse_open_or_recover(&mut self) -> Option<usize> {
            let facts_before = self.strict_parse_errors.len();
            match self.parse_inner() {
                Some(result) => Some(result),
                None => {
                    if self.strict_parse_errors.len() == facts_before {
                        self.record_unexpected_eof(span);
                    }
                    self.pos = self.len();
                    None
                }
            }
        }
    "#;
    assert!(
        unblessed_recovery_functions(fixture).is_empty(),
        "the fact-count-guarded fallback (a fact dominates the recovery exit on every path) \
         must NOT be flagged"
    );
}

#[test]
fn accepts_unconditional_depth0_record_before_scan_recovery() {
    // A scan-to-`>` (or EOF jump) whose strict fact is recorded UNCONDITIONALLY at the
    // block's top level, textually before the recovery exit, dominates — accepted. This is
    // the fixed `find_nested_raw_close` shape (record + advance on the no-close path).
    let fixture = r#"
        fn scan_raw_close_dominated(&mut self) -> Option<usize> {
            let mut p = self.pos;
            while p < self.len() {
                if self.is_close(p) {
                    return Some(p);
                }
                p += 1;
            }
            self.record_expected_token(span);
            self.pos = self.len();
            None
        }
    "#;
    assert!(
        unblessed_recovery_functions(fixture).is_empty(),
        "an unconditional depth-0 strict fact before the recovery exit dominates and must \
         NOT be flagged"
    );
}

#[test]
fn record_close_violation_does_not_bless_a_strict_fact_recovery() {
    // A `record_close_violation` in the SAME block as a recovery primitive must NOT bless
    // it — it pushes a `CloseTagViolation`, not a `SvelteStrictParseError`.
    let fixture = r#"
        fn parse_close_violation_only(&mut self) {
            self.record_close_violation(CloseTagViolationKind::Unclosed, name, span);
            self.pos = self.len();
        }
    "#;
    assert_eq!(
        unblessed_recovery_functions(fixture),
        vec!["parse_close_violation_only".to_string()],
        "`record_close_violation` (a CloseTagViolation, not a strict fact) must not bless a \
         recovery primitive"
    );
}

#[test]
fn accepts_blessed_recovery() {
    let fixture = r#"
        fn parse_blessed(&mut self) {
            // recovers AND records a strict fact in the same block
            self.record_expected_token(span);
            self.pos = self.len();
        }
    "#;
    assert!(
        unblessed_recovery_functions(fixture).is_empty(),
        "a function that records a strict fact alongside its recovery is fine"
    );
}

#[test]
fn accepts_blessed_recovery_in_each_branch() {
    // Each branch with a recovery primitive records its OWN strict fact — all blessed.
    let fixture = r#"
        fn parse_two_blessed_branches(&mut self) {
            if self.cur() == b'/' {
                self.record_nameless_close(span);
                self.pos = self.len();
            } else {
                self.record_expected_token(span);
                self.pos = self.len();
            }
        }
    "#;
    assert!(
        unblessed_recovery_functions(fixture).is_empty(),
        "two branches that each record a strict fact in their own block are fine"
    );
}

#[test]
fn accepts_allowlisted_function() {
    let fixture = r#"
        fn consume_close_tag(&mut self) {
            let mut p = self.pos + 2;
            while p < self.len() && self.at(p) != b'>' {
                p += 1;
            }
            self.pos = (p + 1).min(self.len());
        }
    "#;
    assert!(
        unblessed_recovery_functions(fixture).is_empty(),
        "an allowlisted helper (its caller records the fact) is fine"
    );
}
