//! Regression coverage at the LSP layer: incomplete `<script setup>` member access
//! (`a.`) ships a valid IDE TSX virtual file with correct completion mapping.
//!
//! Drives the REAL LSP document pipeline (`DocumentRegistry::did_open` →
//! `host.upsert` → `ensure_compiled` → `get_ide` → `PositionMapper`) — the exact
//! path `server::nav_features::handle_completion` consumes via
//! `type_provider_context` → `ide_context`.
//!
//! BUG-REPORT.md symptoms these tests guard against regressing:
//!   * "Intellisense is lost": typing `a.` in `<script setup>` → the LSP
//!     compiles the SFC to a `.vue.tsx` virtual file that is shipped to
//!     tsgo/tsserver. Without recovery that virtual file would be STRUCTURALLY
//!     INVALID TSX (the trailing `.` absorbing adjacent synthetic scaffolding),
//!     degrading the language service → "No Suggestions"; these tests assert it
//!     ships VALID TSX.
//!   * "position mapping failed for …": the completion handler maps the
//!     cursor via `merge::carrier_position_to_tsx_offset_validated`; a strict-None at
//!     the zero-width member boundary is by design, but the virtual file around the
//!     mapped region must be well-formed so the completion-boundary fallback has
//!     valid TSX to anchor in.
//!   * "diagnostics show `let` as invalid": the TSX tsgo type-checks must
//!     stay well-formed so a parse error cannot land on the user's `let`.
//!
//! These all share ONE codegen root cause (the verbatim trailing-dot emission in
//! `ide/script/setup.rs`), now handled by the single token-scan recovery path and
//! observed here through the LSP surface rather than the compiler surface.
//!
//! Visibility note: the completion-boundary FALLBACK
//! (`merge::vue_completion_member_boundary_offset`) is `pub(crate)` and cannot be
//! called from an integration test; only the strict public mapper is exercised
//! here. The end-to-end "No Suggestions" needs a live tsgo and the `pub(crate)`
//! `test_harness`; this test pins the ROOT (the broken virtual file the LSP
//! ships) through public APIs.

use std::sync::Arc;

use tower_lsp_server::ls_types::*;
use verter_session::{HostConfig, VerterHost};

use verter_lsp::documents::line_index::LineIndex;
use verter_lsp::documents::provider_projection::ProviderPositionMapper;
use verter_lsp::documents::DocumentRegistry;
use verter_lsp::type_provider::merge;

/// The exact BUG-REPORT case: incomplete member access inside a multi-line arrow.
const BROKEN: &str =
    "<script setup>\nlet a = 1;\n() => {\n  a.\n  return a\n}\n</script>\n<template>\n  <div>{{ a }}</div>\n</template>\n";

/// Positive control: identical SFC WITHOUT the trailing dot (the user's working
/// state per BUG-REPORT.md). Must produce valid TSX and a usable mapper.
const WORKING: &str =
    "<script setup>\nlet a = 1;\n() => {\n  a\n  return a\n}\n</script>\n<template>\n  <div>{{ a }}</div>\n</template>\n";

struct Opened {
    ide_code: Arc<str>,
    mapper: Option<ProviderPositionMapper>,
    vue_li: LineIndex,
    tsx_li: LineIndex,
}

/// Open `source` through the real `DocumentRegistry` and return the IDE TSX +
/// mapper exactly as `ide_context` would hand them to the completion handler.
fn open(source: &str) -> Opened {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let registry = DocumentRegistry::new(host);
    registry.set_encoding(PositionEncodingKind::UTF16);
    let uri: Uri = "file:///proj/src/Index.vue".parse().unwrap();
    let item = TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: source.to_string(),
    };
    let _ = registry.did_open(&item);

    let ide = registry.get_ide(&uri).expect("LSP must produce IDE TSX");
    let vue_li = registry.get(&uri).unwrap().line_index.clone();
    let tsx_li = LineIndex::new(&ide.code, registry.encoding());
    let mapper = registry.get_position_mapper(&uri);
    Opened {
        ide_code: ide.code.clone(),
        mapper,
        vue_li,
        tsx_li,
    }
}

fn oxc_parse_errors(code: &str) -> Vec<String> {
    let alloc = oxc_allocator::Allocator::new();
    let parsed = oxc_parser::Parser::new(&alloc, code, oxc_span::SourceType::tsx()).parse();
    parsed.errors.iter().map(|e| e.to_string()).collect()
}

/// LSP cursor immediately after the last occurrence of `needle` in `source`.
fn cursor_after_last(source: &str, needle: &str) -> Position {
    let idx = source.rfind(needle).expect("needle not found") + needle.len();
    let line = source[..idx].matches('\n').count() as u32;
    let line_start = source[..idx].rfind('\n').map(|p| p + 1).unwrap_or(0);
    Position {
        line,
        character: (idx - line_start) as u32,
    }
}

/// POSITIVE CONTROL: the working SFC must yield an OXC-clean virtual file.
#[test]
fn working_script_setup_ships_valid_virtual_tsx() {
    let o = open(WORKING);
    let errs = oxc_parse_errors(&o.ide_code);
    assert!(
        errs.is_empty(),
        "POSITIVE CONTROL: the working SFC must ship valid TSX, got {errs:?}\n--- TSX ---\n{}",
        o.ide_code
    );
}

/// HEADLINE (the root cause, observed at the LSP layer): the LSP must ship a VALID
/// `.vue.tsx` virtual file to the type provider for the `a.` case. This is the
/// virtual file tsgo/tsserver type-checks and offers completions against; a parse
/// error spanning the cursor would degrade the whole-file language service.
#[test]
fn broken_member_access_ships_valid_virtual_tsx_to_type_provider() {
    let o = open(BROKEN);
    let errs = oxc_parse_errors(&o.ide_code);
    // Diagnostic dump (visible with --nocapture) — the actual virtual file the LSP
    // hands to the type provider for `a.`.
    eprintln!("--- LSP-shipped IDE TSX for `a.` ---\n{}", o.ide_code);
    eprintln!("--- OXC parse errors: {errs:?}");
    assert!(
        errs.is_empty(),
        "REGRESSION: the LSP ships INVALID TSX to tsgo/tsserver for the incomplete `a.` \
         member access (the trailing dot would absorb adjacent synthetic scaffolding). A parse error \
         spanning the completion cursor degrades the whole virtual file → \"No Suggestions\". \
         OXC errors: {errs:?}\n--- TSX ---\n{}",
        o.ide_code
    );
}

/// "spurious `let` invalid diagnostic": the virtual file tsgo
/// type-checks must stay well-formed around the user's `let a = 1;` line. Without
/// recovery the trailing dot would corrupt the statement stream and a parse error
/// would sit ON / right after `let`, surfacing a bogus `let`-invalid diagnostic on
/// the source. This asserts the `let`-bearing body region parses clean.
#[test]
fn broken_member_access_keeps_let_bearing_body_region_valid() {
    let o = open(BROKEN);
    assert!(
        o.ide_code.contains("let a = 1;"),
        "precondition: the user's `let a = 1;` survives into the virtual file"
    );
    // The user's `let` declaration shares the virtual file the type-checker sees; if
    // that file did not parse, a `let`-invalid diagnostic would surface on the source
    // `let`. Correct codegen keeps the body well-formed.
    assert!(
        oxc_parse_errors(&o.ide_code).is_empty(),
        "REGRESSION: the `let a = 1;`-bearing virtual file is not valid TSX, so the \
         type-checker's parse error maps back onto the user's `let`.\n--- TSX ---\n{}",
        o.ide_code
    );
}

/// "position mapping failed for …": the completion handler maps the
/// cursor with `merge::carrier_position_to_tsx_offset_validated`. A strict-None at the
/// zero-width `a.` member boundary is BY DESIGN (the nav_features `position mapping
/// failed` path then uses the completion-boundary fallback). The invariant: the
/// virtual file the fallback anchors in must be VALID TSX, so the receiver `a` maps
/// into well-formed surrounding code rather than a corrupted region.
#[test]
fn broken_member_access_completion_mapping_operates_over_valid_virtual_file() {
    let o = open(BROKEN);
    let mapper = o.mapper.expect("mapper");
    let boundary = cursor_after_last(BROKEN, "a.");

    // The strict validated mapper is None at the zero-width member boundary — the
    // completion handler logs "position mapping failed" and depends entirely on the
    // fragile fallback from here.
    let strict =
        merge::carrier_position_to_tsx_offset_validated(&boundary, &o.vue_li, &mapper, &o.tsx_li);
    eprintln!("strict mapper at `a.` boundary = {strict:?}");

    // The receiver `a` (the token the member access hangs off) — find where it maps
    // in the generated TSX and inspect the surrounding generated text.
    let recv = cursor_after_last("<script setup>\nlet a = 1;\n() => {\n  a", "  a");
    if let Some(gen) =
        mapper.carrier_to_tsx(verter_span::LspPosition::new(recv.line, recv.character))
    {
        if let Some(off) = o.tsx_li.position_to_offset(&Position {
            line: gen.pos.line,
            character: gen.pos.character,
        }) {
            let around = o
                .ide_code
                .get(off.saturating_sub(4) as usize..(off as usize + 12).min(o.ide_code.len()))
                .unwrap_or("");
            eprintln!("receiver `a` maps near generated text: {around:?}");
        }
    }

    // The discriminating, public-API assertion: the virtual file the completion
    // mapping operates over is VALID TSX. (The strict-None at the boundary is by
    // design; the invariant is that the fallback + tsgo have a well-formed file.)
    assert!(
        oxc_parse_errors(&o.ide_code).is_empty(),
        "REGRESSION: completion position mapping for `a.` operates over a corrupt \
         virtual file; the strict mapper is None ({strict:?}) and the fallback can only anchor in \
         broken TSX.\n--- TSX ---\n{}",
        o.ide_code
    );
}

/// Structural invariant: the `a.` recovery keeps the setup body nested
/// inside `___VERTER___TemplateBindingFN` (body AFTER the wrapper opening), the same
/// as the working case. A regression that stranded the body at MODULE scope (before
/// the wrapper) would reintroduce the ownership/snapshot churn seen in the logs.
#[test]
fn broken_member_access_keeps_body_inside_binding_wrapper() {
    let o = open(BROKEN);
    let wrapper_idx = o
        .ide_code
        .find("___VERTER___TemplateBindingFN")
        .expect("wrapper present");
    let body_idx = o.ide_code.find("let a = 1;").expect("user body present");
    assert!(
        body_idx > wrapper_idx,
        "REGRESSION: recovery strands the user's setup body at MODULE scope \
         (before the ___VERTER___TemplateBindingFN wrapper) instead of nesting it inside, unlike \
         the working case.\n--- TSX ---\n{}",
        o.ide_code
    );
}
