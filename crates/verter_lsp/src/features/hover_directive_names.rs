//! D6: directive-NAME hovers — built-in directive documentation and typed
//! custom-directive hovers.
//!
//! The generated TSX lowers or erases directive names (`v-if` is stripped,
//! `v-my-thing` becomes a `runCustomDirective` call), so the provider can
//! never describe the authored name token. Built-ins get Volar-style doc
//! hovers (fail-closed empty definition — there is nothing authored to jump
//! to); custom directives resolve through Vue's `v-my-thing` → `vMyThing`
//! kebab→camel v-prefix rule to the authored setup/import binding for a typed
//! hover and Ctrl+click navigation. Unknown directives stay silent.

use tower_lsp_server::ls_types::{Hover, HoverContents, MarkupContent, MarkupKind};
use verter_session::FileAnalysisSnapshot;

use crate::features::hover::{hover_for_word, VerterHoverResult};

/// Built-in Vue directives with documentation (name-token hover).
/// `model` is owned by the dedicated v-model hover; `slot` by the D3
/// slot-name hover.
const BUILTIN_DIRECTIVE_DOCS: &[(&str, &str)] = &[
    (
        "if",
        "**`v-if`** — Conditionally renders the element when the expression is truthy.\n\n[Built-in directives — Vue.js](https://vuejs.org/api/built-in-directives.html#v-if)",
    ),
    (
        "else-if",
        "**`v-else-if`** — Renders when the previous `v-if` / `v-else-if` chain link was falsy and this expression is truthy.\n\n[Built-in directives — Vue.js](https://vuejs.org/api/built-in-directives.html#v-else-if)",
    ),
    (
        "else",
        "**`v-else`** — Renders when every previous `v-if` / `v-else-if` chain link was falsy.\n\n[Built-in directives — Vue.js](https://vuejs.org/api/built-in-directives.html#v-else)",
    ),
    (
        "for",
        "**`v-for`** — Renders the element once per item of the source list (`item in items`), exposing the loop variable to the element's subtree.\n\n[Built-in directives — Vue.js](https://vuejs.org/api/built-in-directives.html#v-for)",
    ),
    (
        "show",
        "**`v-show`** — Toggles the element's CSS `display` property by the expression's truthiness. The element stays mounted; use `v-if` for conditional lifecycle.\n\n[Built-in directives — Vue.js](https://vuejs.org/api/built-in-directives.html#v-show)",
    ),
    (
        "on",
        "**`v-on`** — Attaches an event listener: `v-on:event=\"handler\"` or the `@event` shorthand, with optional modifiers (`.prevent`, `.stop`, …).\n\n[Built-in directives — Vue.js](https://vuejs.org/api/built-in-directives.html#v-on)",
    ),
    (
        "bind",
        "**`v-bind`** — Dynamically binds an attribute or component prop: `v-bind:attr=\"expr\"` or the `:attr` shorthand.\n\n[Built-in directives — Vue.js](https://vuejs.org/api/built-in-directives.html#v-bind)",
    ),
    (
        "html",
        "**`v-html`** — Updates the element's `innerHTML` with the expression value. The content is inserted as raw HTML — never use it on untrusted input.\n\n[Built-in directives — Vue.js](https://vuejs.org/api/built-in-directives.html#v-html)",
    ),
    (
        "text",
        "**`v-text`** — Updates the element's text content with the expression value (equivalent to `{{ }}` interpolation on the whole content).\n\n[Built-in directives — Vue.js](https://vuejs.org/api/built-in-directives.html#v-text)",
    ),
    (
        "pre",
        "**`v-pre`** — Skips compilation for the element and its children; the raw mustache syntax is preserved as-is.\n\n[Built-in directives — Vue.js](https://vuejs.org/api/built-in-directives.html#v-pre)",
    ),
    (
        "once",
        "**`v-once`** — Renders the element and its children once, then treats them as static and skips all future updates.\n\n[Built-in directives — Vue.js](https://vuejs.org/api/built-in-directives.html#v-once)",
    ),
    (
        "memo",
        "**`v-memo`** — Memoizes a sub-tree and re-renders it only when one of the dependency array's values changes (`v-memo=\"[a, b]\"`).\n\n[Built-in directives — Vue.js](https://vuejs.org/api/built-in-directives.html#v-memo)",
    ),
    (
        "cloak",
        "**`v-cloak`** — Remains on the element until compilation finishes; combine with a `[v-cloak] { display: none }` CSS rule to hide uncompiled mustaches.\n\n[Built-in directives — Vue.js](https://vuejs.org/api/built-in-directives.html#v-cloak)",
    ),
];

/// Directive names with dedicated hovers elsewhere (`model`, `slot`) or the
/// built-in documentation table — everything else is a CUSTOM directive.
fn is_known_builtin_directive(name: &str) -> bool {
    name == "model"
        || name == "slot"
        || BUILTIN_DIRECTIVE_DOCS
            .iter()
            .any(|(builtin, _)| *builtin == name)
}

/// Crate-visible wrapper for the definition path's custom-directive filter.
pub(crate) fn is_known_builtin_directive_pub(name: &str) -> bool {
    is_known_builtin_directive(name)
}

/// The name-token hover region of a directive: from the directive start up to
/// its argument (or to the end of the raw name when arg-less). Argument
/// positions have their own resolution (child props/events/slots), never a
/// directive-name hover.
fn directive_name_region(
    dir: &verter_semantic::analysis::template::TemplateDirective,
) -> (u32, u32) {
    let end = dir
        .arg_span
        .as_ref()
        .map(|span| span.start)
        .unwrap_or(dir.name_end);
    (dir.span.start, end)
}

/// Doc hover for a built-in directive NAME token (`v-if`, `v-for`, `v-bind`, …).
pub fn builtin_directive_name_hover(
    offset: u32,
    analysis: &FileAnalysisSnapshot,
) -> Option<VerterHoverResult> {
    let template = analysis.template.as_deref()?;
    for el in &template.elements {
        for dir in &el.directives {
            let (region_start, region_end) = directive_name_region(dir);
            if offset < region_start || offset >= region_end {
                continue;
            }
            let doc = BUILTIN_DIRECTIVE_DOCS
                .iter()
                .find(|(name, _)| *name == dir.name)
                .map(|(_, doc)| *doc)?;
            return Some(VerterHoverResult {
                hover: Hover {
                    contents: HoverContents::Markup(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: doc.to_string(),
                    }),
                    range: None,
                },
                vue_kind_label: None,
                source_token: None,
            });
        }
    }
    None
}

/// Resolve a custom directive's authored binding name through Vue's
/// registration rule: `v-my-thing` → `vMyThing` (kebab→camel with the `v`
/// prefix); an already-camelized `vMyThing` spelling maps to itself.
pub fn custom_directive_binding_name(directive_name: &str) -> String {
    let mut chars = directive_name.chars();
    if chars.next() == Some('v')
        && chars
            .next()
            .is_some_and(|second| second.is_ascii_uppercase())
    {
        return directive_name.to_string();
    }
    format!("v{}", crate::server::to_pascal_case(directive_name))
}

/// Svelte directive-keyword documentation (D6). Fires on the KEYWORD token
/// (`use`, `transition`, `in`, `out`, `animate`, `bind`, `class`, `style`,
/// `on`); the local name is answered by the provider through the mapped
/// projection. Unrecognised prefixes stay silent.
const SVELTE_DIRECTIVE_KEYWORD_DOCS: &[(&str, &str)] = &[
    (
        "use",
        "**`use:action`** — Calls the action function with the element (and an optional parameter) when the element is mounted; the action may return lifecycle callbacks (`update` / `destroy`).\n\n[Svelte docs — use](https://svelte.dev/docs/svelte-use)",
    ),
    (
        "transition",
        "**`transition:fn`** — Applies an enter/leave transition to the element as it enters and leaves the DOM.\n\n[Svelte docs — transition](https://svelte.dev/docs/svelte-transition)",
    ),
    (
        "in",
        "**`in:fn`** — Applies a transition to the element only as it enters the DOM.\n\n[Svelte docs — in: / out:](https://svelte.dev/docs/svelte-in-out)",
    ),
    (
        "out",
        "**`out:fn`** — Applies a transition to the element only as it leaves the DOM.\n\n[Svelte docs — in: / out:](https://svelte.dev/docs/svelte-in-out)",
    ),
    (
        "animate",
        "**`animate:fn`** — Applies an animation when the element's position changes, typically inside an `{#each}` block when the list reorders.\n\n[Svelte docs — animate](https://svelte.dev/docs/svelte-animate)",
    ),
    (
        "bind",
        "**`bind:property`** — Creates a two-way binding between a component prop (or element property) and a parent value.\n\n[Svelte docs — bind:](https://svelte.dev/docs/svelte-bind)",
    ),
    (
        "class",
        "**`class:name`** — Toggles the `name` CSS class on the element when the expression (or the same-named value, in shorthand form) is truthy.\n\n[Svelte docs — class:](https://svelte.dev/docs/svelte-class)",
    ),
    (
        "style",
        "**`style:property`** — Sets an inline style property on the element (optionally `|important`).\n\n[Svelte docs — style:](https://svelte.dev/docs/svelte-style)",
    ),
    (
        "on",
        "**`on:event`** — Attaches an event listener to the element (legacy directive form; Svelte 5 prefers the `onevent` attribute spelling).\n\n[Svelte docs — on:](https://svelte.dev/docs/svelte-on)",
    ),
];

/// Doc hover for a Svelte directive KEYWORD token (`use`, `transition`,
/// `bind`, …). Local-name positions return `None` — the provider answers
/// those through the mapped projection (the action/transition function).
pub fn svelte_directive_keyword_hover(
    offset: u32,
    analysis: &FileAnalysisSnapshot,
) -> Option<VerterHoverResult> {
    let template = analysis.template.as_deref()?;
    for dir in &template.svelte_directives {
        if offset < dir.span.start || offset >= dir.keyword_end {
            continue;
        }
        let doc = SVELTE_DIRECTIVE_KEYWORD_DOCS
            .iter()
            .find(|(keyword, _)| *keyword == dir.keyword)
            .map(|(_, doc)| *doc)?;
        return Some(VerterHoverResult {
            hover: Hover {
                contents: HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: doc.to_string(),
                }),
                range: None,
            },
            vue_kind_label: None,
            source_token: None,
        });
    }
    None
}

/// Typed hover for a CUSTOM directive NAME token (`v-my-thing`, `v-focus`):
/// the resolved directive binding's hover (setup binding or import), never a
/// fabrication — an unknown directive is silent.
pub fn custom_directive_name_hover(
    offset: u32,
    analysis: &FileAnalysisSnapshot,
) -> Option<VerterHoverResult> {
    let template = analysis.template.as_deref()?;
    for el in &template.elements {
        for dir in &el.directives {
            if is_known_builtin_directive(&dir.name) {
                continue;
            }
            let (region_start, region_end) = directive_name_region(dir);
            if offset < region_start || offset >= region_end {
                continue;
            }
            let binding_name = custom_directive_binding_name(&dir.name);
            return hover_for_word(&binding_name, analysis);
        }
    }
    None
}
