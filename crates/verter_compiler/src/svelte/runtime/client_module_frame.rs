//! The client module-frame emission helpers — the import prelude, the `$.from_html`
//! root-factory hoist, and the backtick-template escaper. Extracted from `client.rs` to
//! keep the emitter module under the file-size guard.

use super::client_imports::{UserImport, UserImportSlot};
use super::helpers::{ImportPlan, RuntimeImport};
use super::html::TemplateFlag;

/// Emit the module imports from the import plan, interleaving the USER imports in the
/// official two-slot order.
///
/// The official import order (pinned svelte@5.56.3) is: the `disclose-version`
/// side-effect import (the leading byte), the flag side-effect imports, each
/// `<script module>` user import in SOURCE ORDER, the `import * as $ from
/// 'svelte/internal/client'` runtime namespace, then each INSTANCE-script user import
/// in SOURCE ORDER. Duplicate imports from the same source stay separate statements
/// (official does not merge them); `with { … }` attributes are preserved. Future
/// NON-import module statements land AFTER the instance imports.
pub(super) fn emit_imports(out: &mut String, imports: &ImportPlan, user_imports: &[UserImport]) {
    if imports.disclose_version {
        out.push_str("import 'svelte/internal/disclose-version';\n");
    }
    if imports.legacy_flag {
        out.push_str("import 'svelte/internal/flags/legacy';\n");
    }
    if imports.async_flag {
        out.push_str("import 'svelte/internal/flags/async';\n");
    }
    if imports.tracing_flag {
        out.push_str("import 'svelte/internal/flags/tracing';\n");
    }
    // `<script module>` user imports, in source order, BEFORE the runtime namespace.
    for import in user_imports {
        if import.slot == UserImportSlot::Module {
            out.push_str(&import.render_statement());
            out.push('\n');
        }
    }
    let ns = match imports.runtime {
        RuntimeImport::Client => "svelte/internal/client",
        RuntimeImport::Server => "svelte/internal/server",
    };
    out.push_str(&format!("import * as $ from '{ns}';\n"));
    // Instance-script user imports, in source order, AFTER the runtime namespace.
    for import in user_imports {
        if import.slot == UserImportSlot::Instance {
            out.push_str(&import.render_statement());
            out.push('\n');
        }
    }
}

/// Emit the root template-factory hoist (`var root = $.from_html(...)`), returning
/// whether the region mounts a multi-root FRAGMENT (vs a single clone-root element).
///
/// The fragment decision is the `TEMPLATE_FRAGMENT` bit ONLY — a flag carrying just
/// `TEMPLATE_USE_IMPORT_NODE` (a lone `<video>`/custom-element template, flag `2`) is
/// still a SINGLE clone-root element: `$.from_html` returns the element, so the walk must
/// take the single-element path (`var video = root();`), NOT the fragment path
/// (`$.first_child(root())` → null on a single element).
pub(super) fn emit_root_hoist(
    out: &mut String,
    root_var: &str,
    html: &str,
    fragment_flag: Option<TemplateFlag>,
) -> bool {
    // ONLY a `$.from_html(...)` clone factory is module-hoisted. A text-first region emits
    // its `$.text(...)` IN-CLOSURE (it is never a hoisted clone factory — calling a text
    // node like `root()` is the X8 bug), and a comment-anchor / standalone region creates
    // its `$.comment()` frame in the body, so neither reaches this hoist.
    let escaped = escape_template_literal(html);
    match fragment_flag {
        Some(flag) => {
            out.push_str(&format!(
                "var {root_var} = $.from_html(`{escaped}`, {});\n",
                flag.literal()
            ));
            flag.is_fragment()
        }
        None => {
            out.push_str(&format!("var {root_var} = $.from_html(`{escaped}`);\n"));
            false
        }
    }
}

/// Escape a string for embedding inside a backtick template literal (the `$.from_html` /
/// `$.text` argument): backslash, backtick, and `${`.
pub(super) fn escape_template_literal(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => out.push_str("\\\\"),
            '`' => out.push_str("\\`"),
            '$' if chars.peek() == Some(&'{') => {
                out.push_str("\\$");
            }
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::svelte::runtime::client_imports::UserImportSpecifier;
    use oxc_allocator::Allocator;
    use oxc_parser::Parser;
    use oxc_span::SourceType;
    use verter_span::Span;

    #[test]
    fn component_import_source_with_quote_emits_parseable_js() {
        // A `.svelte` specifier containing a single quote AND a backslash: the raw
        // `from '<source>'` splice terminates the string literal early, emitting broken
        // JS. The source must route through the JS single-quote serializer so the
        // specifier stays one quote-safe, parseable string literal.
        let imports = ImportPlan {
            disclose_version: true,
            legacy_flag: false,
            async_flag: false,
            tracing_flag: false,
            runtime: RuntimeImport::Client,
        };
        let user_imports = vec![UserImport {
            slot: UserImportSlot::Instance,
            source: "./O'Bri\\en.svelte".to_string(),
            specifiers: vec![UserImportSpecifier::Default {
                local: "Child".to_string(),
            }],
            attributes: Vec::new(),
            span: Span::new(0, 0),
        }];
        let mut out = String::new();
        emit_imports(&mut out, &imports, &user_imports);

        // The emitted module parses cleanly (the quote + backslash escaped).
        let alloc = Allocator::default();
        let parsed = Parser::new(&alloc, &out, SourceType::mjs()).parse();
        assert!(
            !parsed.panicked && parsed.errors.is_empty(),
            "an import specifier containing a quote must emit parseable JS, got:\n{out}\nerrors: {:?}",
            parsed.errors
        );
        // The specifier's quote + backslash are backslash-escaped (the JS serializer),
        // not raw bytes that close the string.
        assert!(
            out.contains("\\'") && out.contains("\\\\"),
            "the specifier quote + backslash must be escaped, got:\n{out}"
        );
        // Negative: the BROKEN raw-splice prefix (`from './O'Bri`) must NOT appear.
        assert!(
            !out.contains("from './O'Bri"),
            "the raw unescaped specifier must not be spliced, got:\n{out}"
        );
    }
}
