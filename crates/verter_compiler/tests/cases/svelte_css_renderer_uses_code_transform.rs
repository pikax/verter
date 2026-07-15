//! Architecture guard: the Svelte scoped-CSS renderer edits through the
//! SHARED [`CodeTransform`] — CodeTransform-SSOT — and no private edit
//! buffer may return to the css tree.
//!
//! Two halves:
//!
//! - POSITIVE: `src/svelte/runtime/css/render.rs` references `CodeTransform`
//!   in REAL code (comments stripped through the shared string-aware
//!   scanner), so the render's edit mechanism is the shared transform — the
//!   same chunk list that generates the css source map — not a claim in
//!   prose.
//! - NEGATIVE: no file under `src/svelte/runtime/css/` (the matcher/render
//!   tree) reintroduces a private edit buffer: the retired buffer type
//!   names are banned outright (assembled at runtime below, so this guard
//!   never carries the banned byte sequences itself and a repo-wide token
//!   sweep stays clean), and so is the private chunk-buffer SHAPE — one
//!   file declaring both an `fn into_string` consumer and a `chunks:`
//!   chunk-list binding/field (the signature of a `magic-string`-style
//!   buffer; the matcher's `JsVal::into_string` and its `ValueChunk` list
//!   live in SEPARATE files and stay legitimate).
//!
//! A DISCRIMINATION self-test proves each verdict predicate flips on the
//! guarded mutation — the render dropping `CodeTransform`, a reintroduced
//! buffer token, a reassembled buffer shape — via inline-string fixtures
//! (the production tree is never edited to prove discrimination).
//!
//! Registered in `CRITICAL_RULE_GUARDS` under "CodeTransform Is the Single
//! Source of Truth".

use super::svelte_guard_support;

use std::fs;
use std::path::{Path, PathBuf};

use svelte_guard_support::strip_rust_comments;

/// The render file that MUST edit through the shared transform.
const RENDER_FILE: &str = "src/svelte/runtime/css/render.rs";

/// The Svelte css matcher/render tree the private-buffer ban covers.
const CSS_TREE: &str = "src/svelte/runtime/css";

/// The retired private-edit-buffer type names banned from the css tree —
/// assembled at runtime so this guard file never contains the banned byte
/// sequences itself.
fn banned_tokens() -> [String; 2] {
    [["Magic", "Buffer"].concat(), ["Buf", "Chunk"].concat()]
}

fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Recursively collect every `.rs` file under `dir` (tests included — a
/// resurrected buffer's unit tests are the same proliferation).
fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(dir).unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display()));
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs(&path, out);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

/// POSITIVE predicate: the code references `CodeTransform` outside comments
/// (string-aware strip, so a `//` inside a literal can never hide real code).
fn references_code_transform(code: &str) -> bool {
    strip_rust_comments(code).contains("CodeTransform")
}

/// NEGATIVE predicate 1: the retired buffer tokens present in real code.
fn banned_buffer_tokens(code: &str) -> Vec<String> {
    let stripped = strip_rust_comments(code);
    banned_tokens()
        .into_iter()
        .filter(|token| stripped.contains(token.as_str()))
        .collect()
}

/// NEGATIVE predicate 2: the private chunk-buffer SHAPE — one file pairing
/// an `fn into_string` consumer with a `chunks:` chunk-list binding/field.
fn has_private_chunk_buffer_shape(code: &str) -> bool {
    let stripped = strip_rust_comments(code);
    stripped.contains("fn into_string") && stripped.contains("chunks:")
}

#[test]
fn css_renderer_edits_through_the_shared_code_transform() {
    let render = crate_root().join(RENDER_FILE);
    let code =
        fs::read_to_string(&render).unwrap_or_else(|e| panic!("read {}: {e}", render.display()));
    assert!(
        references_code_transform(&code),
        "{RENDER_FILE} must edit through the SHARED `CodeTransform` \
         (CodeTransform-SSOT): the token is absent from its real code — the \
         renderer's edit mechanism drifted off the shared transform"
    );
}

#[test]
fn css_tree_reintroduces_no_private_edit_buffer() {
    let tree = crate_root().join(CSS_TREE);
    let mut files = Vec::new();
    collect_rs(&tree, &mut files);
    assert!(
        !files.is_empty(),
        "the css tree scan found no source files — the guard scanned nothing"
    );
    // Scan-set sanity: the render file itself must be in the scanned tree.
    assert!(
        files.iter().any(|p| p.ends_with("render.rs")),
        "the scan set must include the render file"
    );

    let mut violations = Vec::new();
    for path in &files {
        let code =
            fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        for token in banned_buffer_tokens(&code) {
            violations.push(format!("{} references `{token}`", path.display()));
        }
        if has_private_chunk_buffer_shape(&code) {
            violations.push(format!(
                "{} pairs `fn into_string` with a `chunks:` list — the private \
                 chunk-buffer shape",
                path.display()
            ));
        }
    }

    assert!(
        violations.is_empty(),
        "the Svelte css tree must not grow a private edit buffer — generated \
         css mutates through the shared `CodeTransform` checked operations \
         only (CodeTransform-SSOT):\n{}",
        violations.join("\n")
    );
}

// ───────────────────────── discrimination self-tests ─────────────────────────

#[test]
fn positive_predicate_goes_red_if_the_render_dropped_code_transform() {
    // The REAL render source with every `CodeTransform` token renamed — the
    // exact mutation the positive half exists to catch (an off-transform
    // rewrite of the renderer).
    let render = crate_root().join(RENDER_FILE);
    let code =
        fs::read_to_string(&render).unwrap_or_else(|e| panic!("read {}: {e}", render.display()));
    let dropped = code.replace("CodeTransform", "PrivateEditor");
    assert!(
        !references_code_transform(&dropped),
        "renaming every CodeTransform token must flip the positive predicate"
    );
    // A comment-only mention is NOT a pass: the token must be real code.
    assert!(!references_code_transform(
        "// edits go through CodeTransform\nfn render() {}"
    ));
    // And the real file passes (the self-test mirrors the guard's verdict).
    assert!(references_code_transform(&code));
}

#[test]
fn negative_predicate_goes_red_on_a_reintroduced_buffer_token() {
    let [buffer, chunk] = banned_tokens();
    assert_eq!(
        banned_buffer_tokens(&format!(
            "pub(super) struct {buffer}<'a> {{ original: &'a str }}"
        )),
        vec![buffer.clone()],
    );
    assert_eq!(
        banned_buffer_tokens(&format!("struct {chunk} {{ start: u32, end: u32 }}")),
        vec![chunk.clone()],
    );
    assert_eq!(
        banned_buffer_tokens(&format!(
            "let mut buf = {buffer}::new(src); buf.push({chunk}::new(0, 1));"
        )),
        vec![buffer.clone(), chunk],
    );
    // Comment mentions stay clean; comment lookalikes inside strings cannot
    // hide a same-line reference (the shared string-aware strip).
    assert!(
        banned_buffer_tokens(&format!("// a {buffer} port once lived here\nfn ok() {{}}"))
            .is_empty()
    );
    assert_eq!(
        banned_buffer_tokens(&format!(
            "fn f() {{ let url = \"http://x\"; let b = {buffer}::new(url); }}"
        )),
        vec![buffer],
    );
}

#[test]
fn negative_predicate_goes_red_on_a_reassembled_private_buffer_shape() {
    // The shape: one file pairing `fn into_string` with a `chunks:` list.
    assert!(has_private_chunk_buffer_shape(
        "struct Editor<'a> { chunks: Vec<Piece>, src: &'a str }\n\
         impl Editor<'_> { fn into_string(self) -> String { String::new() } }"
    ));
    // Each half ALONE stays legitimate (the matcher's `JsVal::into_string`
    // and its `ValueChunk` list live in separate files).
    assert!(!has_private_chunk_buffer_shape(
        "impl JsVal { fn into_string(self) -> String { self.0 } }"
    ));
    assert!(!has_private_chunk_buffer_shape(
        "fn split(value: &str) { let chunks: Vec<ValueChunk> = Vec::new(); drop(chunks); }"
    ));
    // A comment cannot assemble the shape.
    assert!(!has_private_chunk_buffer_shape(
        "// chunks: kept in a list, then fn into_string builds the output\nfn ok() {}"
    ));
}
