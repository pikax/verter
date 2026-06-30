//! The client module-frame emission helpers — the import prelude, the `$.from_html`
//! root-factory hoist, and the backtick-template escaper. Extracted from `client.rs` to
//! keep the emitter module under the file-size guard.

use super::client_codegen_helpers::js_single_quoted;
use super::client_plan_types::UserImport;
use super::helpers::{ImportPlan, RuntimeImport};
use super::html::TemplateFlag;

/// Emit the module imports from the import plan, then the USER imports in the official
/// slot.
///
/// The official import order is: the `disclose-version` side-effect import (the leading
/// byte), the flag side-effect imports, the `import * as $ from 'svelte/internal/client'`
/// runtime namespace, and FINALLY — immediately after the runtime namespace — each
/// module-scope user import (`import <local> from '<source>'`) in SOURCE ORDER. The
/// admitted set is exactly the `.svelte`-component-default subset
/// ([`UserImport::ComponentDefault`]); every other import form is fail-closed upstream.
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
    let ns = match imports.runtime {
        RuntimeImport::Client => "svelte/internal/client",
        RuntimeImport::Server => "svelte/internal/server",
    };
    out.push_str(&format!("import * as $ from '{ns}';\n"));
    // The user imports, in source order, immediately after the runtime namespace (the
    // pinned svelte@5.56.3 slot — `import Child from './Child.svelte';`).
    for import in user_imports {
        match import {
            UserImport::ComponentDefault { local, source, .. } => {
                // The specifier routes through the JS single-quote serializer so a quote /
                // backslash in the path stays one quote-safe, parseable string literal.
                out.push_str(&format!(
                    "import {local} from {};\n",
                    js_single_quoted(source)
                ));
            }
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
    use crate::svelte::runtime::client_plan_types::UserImport;
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
        let user_imports = vec![UserImport::ComponentDefault {
            local: "Child".to_string(),
            source: "./O'Bri\\en.svelte".to_string(),
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
